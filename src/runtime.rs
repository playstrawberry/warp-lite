use crate::{
    lang,
    model::{self, Config, Id, Location, MAX_PENDING, Pending, Warps},
    store,
};
use sp_travel::runtime::{Rules, Settings, Travels, find, here, id};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, Weak},
};
use strawberry_plugin_api::{
    Context, EventHandler, Player, Plugin, PluginMetadata, Result, Screen, Server,
    command::{
        Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs,
        StringType,
    },
    commands::CommandHandler,
    common::NamedColor,
    events::{
        FailureVerdict, HandlerOptions, HandlerPriority, InventoryClickEvent, InventoryCloseEvent,
        PlayerDeathEvent, PlayerLeaveEvent,
    },
    events_wit::{
        InventoryClickEventData, InventoryCloseEventData, PlayerDeathEventData,
        PlayerLeaveEventData,
    },
    gui::Gui,
    item_stack::ItemStack,
    permission::{Permission, PermissionDefault, PermissionLevel},
    permissions, register_plugin,
    scheduler::{self, SchedulerExt},
    text::TextComponent,
};

const USE: &str = "warp-lite:use";
const MANAGE: &str = "warp-lite:manage";
const DEFAULT_CONFIG: &str = "delay_seconds=3\ncooldown_seconds=10\nmax_warps=128\n";
type Shared = Arc<Mutex<State>>;

struct Menu {
    page: usize,
    names: Vec<String>,
}

struct State {
    dir: PathBuf,
    config: Config,
    warps: Warps,
    /// The waits, cooldowns and teleports (`sp-travel`); `None` before load and after unload.
    travels: Option<Travels<WarpRules>>,
    menus: BTreeMap<Id, Menu>,
    writer: Option<u32>,
    running: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            config: Config::default(),
            warps: Warps::new(),
            travels: None,
            menus: BTreeMap::new(),
            writer: None,
            running: false,
        }
    }
}

fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn permitted(player: &Player, name: &str) -> bool {
    player.has_permission(USE) && player.has_permission(&format!("warp-lite:warp.{name}"))
}

fn component(locale: &str, key: &str, args: &[&str]) -> TextComponent {
    TextComponent::text(&lang::text(locale, key, args))
}

fn say(player: &Player, key: &str, args: &[&str]) {
    player.send_system_message(component(&player.get_locale(), key, args), false);
}

/// warp-lite's part of a travel: when the wait ends the player must still hold the warp's permission and the warp
/// must still be the one he asked for.
struct WarpRules(Weak<Mutex<State>>);

impl Rules for WarpRules {
    fn recheck(&self, player: &Player, pending: &Pending) -> std::result::Result<(), &'static str> {
        let Some(shared) = self.0.upgrade() else {
            return Err("cancelled");
        };
        let allowed = permitted(player, &pending.label);
        let alive = player.get_health() > 0.0;
        let current = here(player);
        let state = lock(&shared);
        model::revalidate(
            pending,
            &current,
            state.warps.get(&pending.label),
            allowed,
            alive,
        )
    }

    fn say(&self, player: &Player, key: &str, args: &[&str]) {
        say(player, key, args);
    }
}

fn begin(
    shared: &Shared,
    server: &Server,
    player: &Player,
    name: &str,
) -> std::result::Result<(), &'static str> {
    if !permitted(player, name) {
        return Err("no_permission");
    }
    let (target, travels) = {
        let state = lock(shared);
        (
            state.warps.get(name).cloned().ok_or("not_found")?,
            state.travels.clone().ok_or("cancelled")?,
        )
    };
    travels.begin(server, player, name, target)
}

#[derive(Clone)]
enum Mutation {
    Set(String, Location),
    Delete(String),
}

fn mutate(
    shared: &Shared,
    server: &Server,
    player: &Player,
    change: Mutation,
) -> std::result::Result<(), &'static str> {
    if !player.has_permission(MANAGE) {
        return Err("no_permission");
    }
    if lock(shared).writer.is_some() {
        return Err("busy");
    }
    let key = id(player);
    let weak: Weak<Mutex<State>> = Arc::downgrade(shared);
    let task = server.schedule_delayed_async_task(1, move |server| {
        let Some(shared) = weak.upgrade() else {
            return;
        };
        let Some(player) = find(&server, key) else {
            lock(&shared).writer = None;
            return;
        };
        if !player.has_permission(MANAGE) {
            lock(&shared).writer = None;
            say(&player, "no_permission", &[]);
            return;
        }
        let result = (|| {
            let (dir, mut warps, max) = {
                let state = lock(&shared);
                if !state.running {
                    return Err("cancelled");
                }
                (
                    state.dir.clone(),
                    state.warps.clone(),
                    state.config.max_warps,
                )
            };
            let (name, message) = match &change {
                Mutation::Set(name, loc) => {
                    if !warps.contains_key(name) && warps.len() >= max {
                        return Err("warp_limit");
                    }
                    warps.insert(name.clone(), loc.clone());
                    (name, "saved")
                }
                Mutation::Delete(name) => {
                    if warps.remove(name).is_none() {
                        return Err("not_found");
                    }
                    (name, "deleted")
                }
            };
            // Save before publishing or acknowledging; failures preserve authoritative memory.
            store::save(&dir, &warps)?;
            lock(&shared).warps = warps;
            say(&player, message, &[name]);
            Ok(())
        })();
        lock(&shared).writer = None;
        if let Err(key) = result {
            say(&player, key, &[]);
        }
    });
    lock(shared).writer = Some(task);
    say(player, "saving", &[]);
    Ok(())
}

fn visible(shared: &Shared, player: &Player) -> Vec<String> {
    let names: Vec<String> = lock(shared).warps.keys().cloned().collect();
    names
        .into_iter()
        .filter(|name| player.has_permission(&format!("warp-lite:warp.{name}")))
        .collect()
}

fn menu(
    shared: &Shared,
    player: &Player,
    requested: usize,
) -> std::result::Result<(), &'static str> {
    if !player.has_permission(USE) {
        return Err("no_permission");
    }
    let key = id(player);
    {
        let state = lock(shared);
        if !state.menus.contains_key(&key) && state.menus.len() >= MAX_PENDING {
            return Err("busy");
        }
    }
    let names = visible(shared, player);
    let (number, shown) = model::page(&names, requested);
    let locale = player.get_locale();
    let gui = Gui::new(
        Screen::Generic9x6,
        component(&locale, "menu_title", &[&(number + 1).to_string()]),
    );
    gui.set_allow_grab_items(false);
    gui.set_allow_put_items(false);
    for (slot, name) in shown.iter().enumerate() {
        let item = ItemStack::new("minecraft:ender_pearl", 1);
        item.set_custom_name(Some(component(&locale, "menu_warp", &[name])));
        item.set_lore(vec![component(&locale, "menu_hint", &[])]);
        gui.set_item(slot as u32, item);
    }
    for (slot, key) in [(45, "previous"), (49, "menu_close"), (53, "next")] {
        let item = ItemStack::new("minecraft:paper", 1);
        item.set_custom_name(Some(component(&locale, key, &[])));
        gui.set_item(slot, item);
    }
    let shown = shown.to_vec();
    player.open_gui(gui);
    // open_gui closes the previous menu synchronously; publish the new session afterwards.
    lock(shared).menus.insert(
        key,
        Menu {
            page: number,
            names: shown,
        },
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum Action {
    Set,
    Delete,
    Warp,
    List,
    Menu,
}

struct Run(Shared, Action);
impl CommandHandler for Run {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> std::result::Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            return Err(CommandError::CommandFailed(component(
                "en",
                "players_only",
                &[],
            )));
        };
        let result = (|| {
            let arg = match args.get_value("name") {
                Arg::Simple(s) if !s.is_empty() => Some(s),
                _ => None,
            };
            match self.1 {
                Action::Menu => menu(&self.0, &player, 0),
                Action::List => {
                    if !player.has_permission(USE) {
                        return Err("no_permission");
                    }
                    let names = visible(&self.0, &player);
                    let joined = names.join(", ");
                    say(
                        &player,
                        if names.is_empty() { "empty" } else { "list" },
                        &[&joined],
                    );
                    Ok(())
                }
                Action::Warp if arg.is_none() => menu(&self.0, &player, 0),
                action => {
                    let name = model::name(arg.as_deref().ok_or("usage")?).ok_or("invalid_name")?;
                    match action {
                        Action::Set => {
                            let loc = here(&player);
                            if !loc.valid() {
                                return Err("invalid_location");
                            }
                            mutate(&self.0, &server, &player, Mutation::Set(name, loc))
                        }
                        Action::Delete => mutate(&self.0, &server, &player, Mutation::Delete(name)),
                        Action::Warp => begin(&self.0, &server, &player, &name),
                        _ => unreachable!(),
                    }
                }
            }
        })();
        result.map(|()| 1).map_err(|key| {
            CommandError::CommandFailed(
                component(&player.get_locale(), key, &[]).color_named(NamedColor::Red),
            )
        })
    }
}

struct Events(Shared);

// A waiting travel of a player who leaves or dies is dropped by `sp-travel`; the menu session is ours.
fn forget(shared: &Shared, player: &Player) {
    let key = id(player);
    lock(shared).menus.remove(&key);
}

fn has_transient(shared: &Shared) -> bool {
    !lock(shared).menus.is_empty()
}

impl EventHandler<PlayerLeaveEvent> for Events {
    fn handle(&self, _server: Server, event: PlayerLeaveEventData) -> PlayerLeaveEventData {
        if has_transient(&self.0) {
            forget(&self.0, &event.player);
        }
        event
    }
}

impl EventHandler<PlayerDeathEvent> for Events {
    fn handle(&self, _server: Server, event: PlayerDeathEventData) -> PlayerDeathEventData {
        if has_transient(&self.0) {
            forget(&self.0, &event.player);
        }
        event
    }
}

impl EventHandler<InventoryCloseEvent> for Events {
    fn handle(&self, _server: Server, event: InventoryCloseEventData) -> InventoryCloseEventData {
        if !lock(&self.0).menus.is_empty() {
            let key = id(&event.player);
            lock(&self.0).menus.remove(&key);
        }
        event
    }
}

impl EventHandler<InventoryClickEvent> for Events {
    fn handle(
        &self,
        server: Server,
        mut event: InventoryClickEventData,
    ) -> InventoryClickEventData {
        if lock(&self.0).menus.is_empty() {
            return event;
        }
        let key = id(&event.player);
        let choice = {
            let state = lock(&self.0);
            let Some(menu) = state.menus.get(&key) else {
                return event;
            };
            let name = usize::try_from(event.raw_slot)
                .ok()
                .and_then(|slot| menu.names.get(slot))
                .cloned();
            (menu.page, name)
        };
        event.cancelled = true;
        let result = match event.raw_slot {
            45 => menu(&self.0, &event.player, choice.0.saturating_sub(1)),
            53 => menu(&self.0, &event.player, choice.0.saturating_add(1)),
            49 => {
                // This SDK closes the vanilla menu with Escape.
                say(&event.player, "close_hint", &[]);
                Ok(())
            }
            _ => choice
                .1
                .map_or(Ok(()), |name| begin(&self.0, &server, &event.player, &name)),
        };
        if let Err(key) = result {
            say(&event.player, key, &[]);
        }
        event
    }
}

struct WarpLite {
    state: Shared,
}

impl Plugin for WarpLite {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "warp-lite".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Strawberry Interactive LLC".into()],
            description: lang::text("en", "description", &[]),
            dependencies: vec![],
            permissions: vec![
                permissions::FS_READ_DATA.into(),
                permissions::FS_WRITE_DATA.into(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> Result<()> {
        let dir = PathBuf::from(context.get_data_folder());
        std::fs::create_dir_all(&dir).map_err(|_| lang::text("en", "data_error", &[]))?;
        let config_path = dir.join("config.properties");
        let text =
            store::read_optional(&config_path, 4096).map_err(|key| lang::text("en", key, &[]))?;
        if text.is_none() {
            std::fs::write(&config_path, DEFAULT_CONFIG)
                .map_err(|_| lang::text("en", "save_error", &[]))?;
        }
        let config = Config::parse(text.as_deref().unwrap_or(DEFAULT_CONFIG))
            .map_err(|key| lang::text("en", key, &[]))?;
        let warps = store::load(&dir).map_err(|key| lang::text("en", key, &[]))?;
        // Moves and hits reach the script only when a travel waits (`Travels::register`): a server without a
        // delay has nothing to cancel, and the checks at the moment of the travel still refuse a player who moved.
        let travels = Travels::new(
            WarpRules(Arc::downgrade(&self.state)),
            Settings {
                delay_seconds: config.delay_seconds,
                cooldown_seconds: config.cooldown_seconds,
                look: true,
            },
        );
        travels.register(&context)?;
        *lock(&self.state) = State {
            dir,
            config,
            warps,
            travels: Some(travels),
            running: true,
            ..State::default()
        };
        for (node, description, default) in [
            (USE, "permission_use", PermissionDefault::Allow),
            (
                MANAGE,
                "permission_manage",
                PermissionDefault::Op(PermissionLevel::Two),
            ),
        ] {
            context.register_permission(&Permission {
                node: node.into(),
                description: lang::text("en", description, &[]),
                default,
                children: vec![],
            })?;
        }
        let word = ArgumentType::String(StringType::SingleWord);
        for (label, action, permission, desc) in [
            ("setwarp", Action::Set, MANAGE, "command_set"),
            ("delwarp", Action::Delete, MANAGE, "command_delete"),
            ("warp", Action::Warp, USE, "command_warp"),
            ("warps", Action::List, USE, "command_list"),
            ("warpmenu", Action::Menu, USE, "command_menu"),
        ] {
            let run = || Run(Arc::clone(&self.state), action);
            let mut command =
                Command::new(&[label.into()], &lang::text("en", desc, &[])).execute(run());
            if matches!(action, Action::Set | Action::Delete | Action::Warp) {
                command = command.then(CommandNode::argument("name", &word).execute(run()));
            }
            if matches!(action, Action::List) {
                command = command
                    .then(CommandNode::literal("list").execute(run()))
                    .then(
                        CommandNode::literal("menu")
                            .execute(Run(Arc::clone(&self.state), Action::Menu)),
                    );
            }
            context.register_command(command, permission);
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
                    Events(Arc::clone(&self.state)),
                    observe.clone(),
                )?;
            };
        }
        observer!(PlayerLeaveEvent);
        observer!(PlayerDeathEvent);
        observer!(InventoryCloseEvent);
        context.register_event_handler_with_options::<InventoryClickEvent, _>(
            Events(Arc::clone(&self.state)),
            HandlerOptions {
                priority: HandlerPriority::Highest,
                blocking: true,
                ignore_cancelled: false,
                on_failure: FailureVerdict::Allow,
            },
        )?;
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<()> {
        let (travels, writer) = {
            let mut state = lock(&self.state);
            state.running = false;
            state.menus.clear();
            (state.travels.take(), state.writer.take())
        };
        if let Some(travels) = travels {
            travels.stop();
        }
        if let Some(task) = writer {
            scheduler::cancel_task(task);
        }
        Ok(())
    }
}

register_plugin!(WarpLite);
