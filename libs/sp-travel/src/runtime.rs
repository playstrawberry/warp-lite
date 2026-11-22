//! The wait's timer and the events that cancel it (the plugin target only).
//!
//! A script makes one [`Travels`] at load, hands it every request it accepted ([`Travels::begin`]) and gets the
//! player moved when the wait ends: rechecked by the script ([`Rules::recheck`]), alive, still where he asked from,
//! then through the one teleport call of [`crate::safety`], with the arrival checked before anything is said.
//!
//! What it costs the server: one one-shot task while somebody waits, nothing otherwise. Only a script with a wait
//! listens to moves and damage; without a wait no event handler is registered at all (a request travels at the
//! next tick, and the checks at that moment refuse a player who moved, died or left).
use crate::{
    Id, Location,
    queue::{Pending, Queue},
    safety,
};
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use strawberry_plugin_api::{
    Context, EventHandler, Player, Result, Server,
    events::{
        EntityDamageEvent, FailureVerdict, HandlerOptions, HandlerPriority, PlayerDeathEvent,
        PlayerLeaveEvent, PlayerMoveEvent, PlayerTeleportEvent,
    },
    events_wit::{
        EntityDamageEventData, PlayerDeathEventData, PlayerLeaveEventData, PlayerMoveEventData,
        PlayerTeleportEventData,
    },
    scheduler::{self, SchedulerExt},
    uuid::Uuid,
};

/// What a script decides about its own travels.
pub trait Rules: Send + Sync + 'static {
    /// The wait is over: may this player still go there (his permission, the destination unchanged)? `Err(key)`
    /// refuses with that text of the script. Asked on the player as he is now, never on what he was at the request.
    fn recheck(&self, player: &Player, pending: &Pending) -> std::result::Result<(), &'static str>;
    /// One line to the player: `key` of the script's lang files with `args`.
    fn say(&self, player: &Player, key: &str, args: &[&str]);
    /// The player arrived (after his position was checked).
    fn arrived(&self, _player: &Player, _pending: &Pending) {}
}

/// How a script's travels behave.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Seconds a request waits; a step, a hit, a death or another teleport in that time cancels it. 0: the next tick.
    pub delay_seconds: u64,
    /// Seconds after an arrival before the next request is accepted. 0: none.
    pub cooldown_seconds: u64,
    /// Whether a loaded destination is inspected first ([`crate::safety`]).
    pub look: bool,
}

struct State {
    queue: Queue,
    clock: Instant,
    timer: Option<u32>,
    running: bool,
}

struct Inner<R> {
    rules: R,
    settings: Settings,
    state: Mutex<State>,
}

/// A script's travels. Clones share one queue.
pub struct Travels<R>(Arc<Inner<R>>);

impl<R> Clone for Travels<R> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

/// The player's id.
pub fn id(player: &Player) -> Id {
    let uuid = player.get_id();
    (uuid.high, uuid.low)
}

/// The online player with this id.
pub fn find(server: &Server, key: Id) -> Option<Player> {
    server.get_player_by_uuid(Uuid {
        high: key.0,
        low: key.1,
    })
}

/// Where the player is now.
pub fn here(player: &Player) -> Location {
    Location {
        world: player.get_world().get_name(),
        pos: player.get_position(),
        yaw: player.get_yaw(),
        pitch: player.get_pitch(),
    }
}

fn lock<R>(inner: &Inner<R>) -> MutexGuard<'_, State> {
    inner
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now(state: &State) -> u64 {
    u64::try_from(state.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl<R: Rules> Travels<R> {
    pub fn new(rules: R, settings: Settings) -> Self {
        Self(Arc::new(Inner {
            rules,
            settings,
            state: Mutex::new(State {
                queue: Queue::default(),
                clock: Instant::now(),
                timer: None,
                running: true,
            }),
        }))
    }

    /// The script's rules.
    pub fn rules(&self) -> &R {
        &self.0.rules
    }

    pub fn settings(&self) -> Settings {
        self.0.settings
    }

    /// Accepts a request the script already allowed. `Err(key)` when the player is dead, already waits, cools
    /// down, or the queue is full; nothing is said then, the caller answers with the key.
    pub fn begin(
        &self,
        server: &Server,
        player: &Player,
        label: &str,
        target: Location,
    ) -> std::result::Result<(), &'static str> {
        let key = id(player);
        let start = here(player);
        if !start.valid() || player.get_health() <= 0.0 {
            return Err("cancelled");
        }
        let entity = player.as_entity().get_id() as i32;
        let delay = self.0.settings.delay_seconds;
        {
            let mut state = lock(&self.0);
            if !state.running {
                return Err("cancelled");
            }
            let time = now(&state);
            state.queue.begin(
                key,
                Pending {
                    label: label.into(),
                    target,
                    start,
                    entity,
                    due: time.saturating_add(delay.saturating_mul(1000)),
                },
                time,
            )?;
        }
        if delay > 0 {
            self.0
                .rules
                .say(player, "waiting", &[label, &delay.to_string()]);
        }
        self.arm(server);
        Ok(())
    }

    // Only one one-shot deadline task, and at most BATCH completions in one callback.
    fn arm(&self, server: &Server) {
        let delay = {
            let state = lock(&self.0);
            if state.timer.is_some() || !state.running {
                return;
            }
            let Some(due) = state.queue.pending.values().map(|p| p.due).min() else {
                return;
            };
            due.saturating_sub(now(&state)).div_ceil(50).max(1)
        };
        let weak = Arc::downgrade(&self.0);
        let task = server.schedule_delayed_task(delay, move |server| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let travels = Travels(inner);
            let due = {
                let mut state = lock(&travels.0);
                state.timer = None;
                let time = now(&state);
                state.queue.due(time)
            };
            for (key, pending) in due {
                let Some(player) = find(&server, key) else {
                    continue;
                };
                let rules = &travels.0.rules;
                let result = rules
                    .recheck(&player, &pending)
                    .and_then(|()| pending.still(&here(&player), player.get_health() > 0.0))
                    .and_then(|()| {
                        safety::teleport(&server, &player, &pending.target, travels.0.settings.look)
                    });
                match result {
                    Ok(()) => {
                        {
                            let mut state = lock(&travels.0);
                            let time = now(&state);
                            state
                                .queue
                                .succeeded(key, time, travels.0.settings.cooldown_seconds);
                        }
                        rules.say(&player, "teleported", &[&pending.label]);
                        rules.arrived(&player, &pending);
                    }
                    Err(key) => rules.say(&player, key, &[]),
                }
            }
            travels.arm(&server);
        });
        lock(&self.0).timer = Some(task);
    }

    fn stop_empty_timer(&self) {
        let task = {
            let mut state = lock(&self.0);
            if state.queue.pending.is_empty() {
                state.timer.take()
            } else {
                None
            }
        };
        if let Some(task) = task {
            scheduler::cancel_task(task);
        }
    }

    /// Drops the player's waiting request without a word (he left, died or was teleported by something else).
    pub fn forget(&self, player: &Player) {
        let key = id(player);
        lock(&self.0).queue.pending.remove(&key);
        self.stop_empty_timer();
    }

    fn waiting(&self) -> bool {
        !lock(&self.0).queue.pending.is_empty()
    }

    /// Registers the events that cancel a wait. A script without a wait registers none: the engine then never
    /// calls it for a move or a hit.
    pub fn register(&self, context: &Context) -> Result<()> {
        if self.0.settings.delay_seconds == 0 {
            return Ok(());
        }
        let observe = HandlerOptions {
            priority: HandlerPriority::Monitor,
            blocking: true,
            ignore_cancelled: true,
            on_failure: FailureVerdict::Allow,
        };
        macro_rules! observer {
            ($event:ty) => {
                context.register_event_handler_with_options::<$event, _>(
                    Watch(self.clone()),
                    observe.clone(),
                )?;
            };
        }
        observer!(PlayerMoveEvent);
        observer!(EntityDamageEvent);
        observer!(PlayerLeaveEvent);
        observer!(PlayerDeathEvent);
        observer!(PlayerTeleportEvent);
        Ok(())
    }

    /// Unload: nothing waits, nothing cools down, the timer is gone, and no later request is accepted.
    pub fn stop(&self) {
        let timer = {
            let mut state = lock(&self.0);
            state.running = false;
            state.queue.pending.clear();
            state.queue.cooldowns.clear();
            state.timer.take()
        };
        if let Some(task) = timer {
            scheduler::cancel_task(task);
        }
    }
}

struct Watch<R>(Travels<R>);

impl<R: Rules> EventHandler<PlayerMoveEvent> for Watch<R> {
    fn handle(&self, _server: Server, event: PlayerMoveEventData) -> PlayerMoveEventData {
        if event.cancelled || !self.0.waiting() {
            return event;
        }
        let key = id(&event.player);
        if !lock(&self.0.0).queue.pending.contains_key(&key) {
            return event;
        }
        let world = event.player.get_world().get_name();
        let cancelled = {
            let mut state = lock(&self.0.0);
            if state
                .queue
                .pending
                .get(&key)
                .is_some_and(|p| crate::moved(&p.start, &world, event.to_position))
            {
                state.queue.pending.remove(&key);
                true
            } else {
                false
            }
        };
        if cancelled {
            self.0.stop_empty_timer();
            self.0.0.rules.say(&event.player, "moved", &[]);
        }
        event
    }
}

impl<R: Rules> EventHandler<EntityDamageEvent> for Watch<R> {
    fn handle(&self, server: Server, event: EntityDamageEventData) -> EntityDamageEventData {
        if event.cancelled || !event.damage.is_finite() || event.damage <= 0.0 {
            return event;
        }
        let key = { lock(&self.0.0).queue.cancel_entity(event.entity_id) };
        if let Some(key) = key {
            self.0.stop_empty_timer();
            if let Some(player) = find(&server, key) {
                self.0.0.rules.say(&player, "damaged", &[]);
            }
        }
        event
    }
}

impl<R: Rules> EventHandler<PlayerLeaveEvent> for Watch<R> {
    fn handle(&self, _server: Server, event: PlayerLeaveEventData) -> PlayerLeaveEventData {
        if self.0.waiting() {
            self.0.forget(&event.player);
        }
        event
    }
}

impl<R: Rules> EventHandler<PlayerDeathEvent> for Watch<R> {
    fn handle(&self, _server: Server, event: PlayerDeathEventData) -> PlayerDeathEventData {
        if self.0.waiting() {
            self.0.forget(&event.player);
        }
        event
    }
}

impl<R: Rules> EventHandler<PlayerTeleportEvent> for Watch<R> {
    fn handle(&self, _server: Server, event: PlayerTeleportEventData) -> PlayerTeleportEventData {
        if self.0.waiting() {
            self.0.forget(&event.player);
        }
        event
    }
}
