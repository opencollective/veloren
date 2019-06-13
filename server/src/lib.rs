#![feature(drain_filter)]

pub mod client;
pub mod cmd;
pub mod error;
pub mod input;

// Reexports
pub use crate::{error::Error, input::Input};

use crate::{
    client::{Client, Clients},
    cmd::CHAT_COMMANDS,
};
use common::{
    comp,
    msg::{chat::MAX_MSG_LEN, ClientMsg, ClientState, RequestStateError, ServerInfo, ServerMsg},
    net::PostOffice,
    state::{State, Uid},
    terrain::{TerrainChunk, TerrainChunkSize},
    vol::VolSize,
};
use specs::{
    join::Join, saveload::MarkedBuilder, world::EntityBuilder as EcsEntityBuilder, Builder,
    Entity as EcsEntity,
};
use std::{
    collections::HashSet,
    i32,
    net::SocketAddr,
    sync::{mpsc, Arc},
    time::Duration,
};
use threadpool::ThreadPool;
use vek::*;
use world::World;

const CLIENT_TIMEOUT: f64 = 20.0; // Seconds

const DEFAULT_WORLD_SEED: u32 = 1337;

pub enum Event {
    ClientConnected {
        entity: EcsEntity,
    },
    ClientDisconnected {
        entity: EcsEntity,
    },
    Chat {
        entity: Option<EcsEntity>,
        msg: String,
    },
}

#[derive(Copy, Clone)]
struct SpawnPoint(Vec3<f32>);

pub struct Server {
    state: State,
    world: Arc<World>,

    postoffice: PostOffice<ServerMsg, ClientMsg>,
    clients: Clients,

    thread_pool: ThreadPool,
    chunk_tx: mpsc::Sender<(Vec2<i32>, TerrainChunk)>,
    chunk_rx: mpsc::Receiver<(Vec2<i32>, TerrainChunk)>,
    pending_chunks: HashSet<Vec2<i32>>,

    server_info: ServerInfo,
}

impl Server {
    /// Create a new `Server` bound to the default socket.
    #[allow(dead_code)]
    pub fn new() -> Result<Self, Error> {
        Self::bind(SocketAddr::from(([0; 4], 59003)))
    }

    /// Create a new server bound to the given socket.
    #[allow(dead_code)]
    pub fn bind<A: Into<SocketAddr>>(addrs: A) -> Result<Self, Error> {
        let (chunk_tx, chunk_rx) = mpsc::channel();

        let mut state = State::new();
        state
            .ecs_mut()
            .add_resource(SpawnPoint(Vec3::new(16_384.0, 16_384.0, 280.0)));

        let mut this = Self {
            state,
            world: Arc::new(World::generate(DEFAULT_WORLD_SEED)),

            postoffice: PostOffice::bind(addrs.into())?,
            clients: Clients::empty(),

            thread_pool: threadpool::Builder::new()
                .thread_name("veloren-worker".into())
                .build(),
            chunk_tx,
            chunk_rx,
            pending_chunks: HashSet::new(),

            server_info: ServerInfo {
                name: "Server name".to_owned(),
                description: "This is the best Veloren server.".to_owned(),
            },
        };

        /*
        for i in 0..4 {
            this.create_npc(
                "Tobermory".to_owned(),
                comp::Body::Humanoid(comp::HumanoidBody::random()),
            )
            .with(comp::Actions::default())
            .with(comp::Agent::Wanderer(Vec2::zero()))
            .build();
        }
        */

        Ok(this)
    }

    /// Get a reference to the server's game state.
    #[allow(dead_code)]
    pub fn state(&self) -> &State {
        &self.state
    }
    /// Get a mutable reference to the server's game state.
    #[allow(dead_code)]
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// Get a reference to the server's world.
    #[allow(dead_code)]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Build a non-player character.
    #[allow(dead_code)]
    pub fn create_npc(
        &mut self,
        pos: comp::phys::Pos,
        name: String,
        body: comp::Body,
    ) -> EcsEntityBuilder {
        self.state
            .ecs_mut()
            .create_entity_synced()
            .with(pos)
            .with(comp::phys::Vel(Vec3::zero()))
            .with(comp::phys::Ori(Vec3::unit_y()))
            .with(comp::Control::default())
            .with(comp::AnimationInfo::default())
            .with(comp::Actor::Character { name, body })
            .with(comp::Stats::default())
            .with(comp::phys::ForceUpdate)
    }

    pub fn create_player_character(
        state: &mut State,
        entity: EcsEntity,
        client: &mut Client,
        name: String,
        body: comp::Body,
    ) {
        let spawn_point = state.ecs().read_resource::<SpawnPoint>().0;

        state.write_component(entity, comp::Actor::Character { name, body });
        state.write_component(entity, comp::Stats::default());
        state.write_component(entity, comp::AnimationInfo::default());
        state.write_component(entity, comp::phys::Pos(spawn_point));
        state.write_component(entity, comp::phys::Vel(Vec3::zero()));
        state.write_component(entity, comp::phys::Ori(Vec3::unit_y()));
        // Make sure physics are accepted.
        state.write_component(entity, comp::phys::ForceUpdate);

        // Tell the client its request was successful.
        client.allow_state(ClientState::Character);
    }

    /// Execute a single server tick, handle input and update the game state by the given duration.
    #[allow(dead_code)]
    pub fn tick(&mut self, input: Input, dt: Duration) -> Result<Vec<Event>, Error> {
        // This tick function is the centre of the Veloren universe. Most server-side things are
        // managed from here, and as such it's important that it stays organised. Please consult
        // the core developers before making significant changes to this code. Here is the
        // approximate order of things. Please update it as this code changes.
        //
        // 1) Collect input from the frontend, apply input effects to the state of the game
        // 2) Go through any events (timer-driven or otherwise) that need handling and apply them
        //    to the state of the game
        // 3) Go through all incoming client network communications, apply them to the game state
        // 4) Perform a single LocalState tick (i.e: update the world and entities in the world)
        // 5) Go through the terrain update queue and apply all changes to the terrain
        // 6) Send relevant state updates to all clients
        // 7) Finish the tick, passing control of the main thread back to the frontend

        // 1) Build up a list of events for this frame, to be passed to the frontend.
        let mut frontend_events = Vec::new();

        // If networking has problems, handle them.
        if let Some(err) = self.postoffice.error() {
            return Err(err.into());
        }

        // 2)

        // 3) Handle inputs from clients
        frontend_events.append(&mut self.handle_new_connections()?);
        frontend_events.append(&mut self.handle_new_messages()?);

        // 4) Tick the client's LocalState.
        self.state.tick(dt);

        // Tick the world
        self.world.tick(dt);

        // Sync deaths.
        let ecs = &self.state.ecs();
        let clients = &mut self.clients;
        let todo_kill = (&ecs.entities(), &ecs.read_storage::<comp::Dying>())
            .join()
            .map(|(entity, dying)| {
                // Chat message
                if let Some(player) = ecs.read_storage::<comp::Player>().get(entity) {
                    let msg = if let comp::HealthSource::Attack { by } = dying.cause {
                        ecs.entity_from_uid(by.into()).and_then(|attacker| {
                            ecs.read_storage::<comp::Player>()
                                .get(attacker)
                                .map(|attacker_alias| {
                                    format!(
                                        "{} was killed by {}",
                                        &player.alias, &attacker_alias.alias
                                    )
                                })
                        })
                    } else {
                        None
                    }
                    .unwrap_or(format!("{} died", &player.alias));

                    clients.notify_registered(ServerMsg::Chat(msg));
                }

                entity
            })
            .collect::<Vec<_>>();

        // Actually kill them
        for entity in todo_kill {
            if let Some(client) = self.clients.get_mut(&entity) {
                self.state
                    .write_component(entity, comp::phys::Vel(Vec3::zero()));
                self.state.write_component(entity, comp::phys::ForceUpdate);
                client.force_state(ClientState::Dead);
            } else {
                self.state.ecs_mut().delete_entity_synced(entity);
                continue;
            }
        }

        // Handle respawns
        let todo_respawn = (
            &self.state.ecs().entities(),
            &self.state.ecs().read_storage::<comp::Respawning>(),
        )
            .join()
            .map(|(entity, _)| entity)
            .collect::<Vec<EcsEntity>>();

        for entity in todo_respawn {
            if let Some(client) = self.clients.get_mut(&entity) {
                client.allow_state(ClientState::Character);
                self.state.write_component(entity, comp::Stats::default());
                self.state
                    .ecs_mut()
                    .write_storage::<comp::phys::Pos>()
                    .get_mut(entity)
                    .map(|pos| pos.0.z += 100.0);
                self.state
                    .write_component(entity, comp::phys::Vel(Vec3::zero()));
                self.state.write_component(entity, comp::phys::ForceUpdate);
            }
        }

        // 5) Fetch any generated `TerrainChunk`s and insert them into the terrain.
        // Also, send the chunk data to anybody that is close by.
        if let Ok((key, chunk)) = self.chunk_rx.try_recv() {
            // Send the chunk to all nearby players.
            for (entity, view_distance, pos) in (
                &self.state.ecs().entities(),
                &self.state.ecs().read_storage::<comp::Player>(),
                &self.state.ecs().read_storage::<comp::phys::Pos>(),
            )
                .join()
                .filter_map(|(entity, player, pos)| {
                    player.view_distance.map(|vd| (entity, vd, pos))
                })
            {
                let chunk_pos = self.state.terrain().pos_key(pos.0.map(|e| e as i32));
                let dist = (Vec2::from(chunk_pos) - Vec2::from(key))
                    .map(|e: i32| e.abs())
                    .reduce_max() as u32;

                if dist <= view_distance {
                    self.clients.notify(
                        entity,
                        ServerMsg::TerrainChunkUpdate {
                            key,
                            chunk: Box::new(chunk.clone()),
                        },
                    );
                }
            }

            self.state.insert_chunk(key, chunk);
            self.pending_chunks.remove(&key);
        }

        // Remove chunks that are too far from players.
        let mut chunks_to_remove = Vec::new();
        self.state.terrain().iter().for_each(|(key, _)| {
            let mut should_drop = true;

            // For each player with a position, calculate the distance.
            for (player, pos) in (
                &self.state.ecs().read_storage::<comp::Player>(),
                &self.state.ecs().read_storage::<comp::phys::Pos>(),
            )
                .join()
            {
                let chunk_pos = self.state.terrain().pos_key(pos.0.map(|e| e as i32));
                let dist = Vec2::from(chunk_pos - key)
                    .map(|e: i32| e.abs() as u32)
                    .reduce_max();

                if player.view_distance.map(|vd| dist <= vd).unwrap_or(false) {
                    should_drop = false;
                    break;
                }
            }

            if should_drop {
                chunks_to_remove.push(key);
            }
        });
        for key in chunks_to_remove {
            self.state.remove_chunk(key);
        }

        // 6) Synchronise clients with the new state of the world.
        self.sync_clients();

        // 7) Finish the tick, pass control back to the frontend.

        // Cleanup
        let ecs = self.state.ecs_mut();
        for entity in ecs.entities().join() {
            ecs.write_storage::<comp::Dying>().remove(entity);
            ecs.write_storage::<comp::Respawning>().remove(entity);
        }

        Ok(frontend_events)
    }

    /// Clean up the server after a tick.
    #[allow(dead_code)]
    pub fn cleanup(&mut self) {
        // Cleanup the local state
        self.state.cleanup();
    }

    /// Handle new client connections.
    fn handle_new_connections(&mut self) -> Result<Vec<Event>, Error> {
        let mut frontend_events = Vec::new();

        for mut postbox in self.postoffice.new_postboxes() {
            let entity = self.state.ecs_mut().create_entity_synced().build();
            let mut client = Client {
                client_state: ClientState::Connected,
                postbox,
                last_ping: self.state.get_time(),
            };

            // Return the state of the current world (all of the components that Sphynx tracks).
            client.notify(ServerMsg::InitialSync {
                ecs_state: self.state.ecs().gen_state_package(),
                entity_uid: self.state.ecs().uid_from_entity(entity).unwrap().into(), // Can't fail.
                server_info: self.server_info.clone(),
            });

            self.clients.add(entity, client);

            frontend_events.push(Event::ClientConnected { entity });
        }

        Ok(frontend_events)
    }

    /// Handle new client messages.
    fn handle_new_messages(&mut self) -> Result<Vec<Event>, Error> {
        let mut frontend_events = Vec::new();

        let state = &mut self.state;
        let mut new_chat_msgs = Vec::new();
        let mut disconnected_clients = Vec::new();
        let mut requested_chunks = Vec::new();

        self.clients.remove_if(|entity, client| {
            let mut disconnect = false;
            let new_msgs = client.postbox.new_messages();

            // Update client ping.
            if new_msgs.len() > 0 {
                client.last_ping = state.get_time();

                // Process incoming messages.
                for msg in new_msgs {
                    match msg {
                        ClientMsg::RequestState(requested_state) => match requested_state {
                            ClientState::Connected => disconnect = true, // Default state
                            ClientState::Registered => match client.client_state {
                                // Use ClientMsg::Register instead.
                                ClientState::Connected => {
                                    client.error_state(RequestStateError::WrongMessage)
                                }
                                ClientState::Registered => {
                                    client.error_state(RequestStateError::Already)
                                }
                                ClientState::Spectator
                                | ClientState::Character
                                | ClientState::Dead => client.allow_state(ClientState::Registered),
                                ClientState::Pending => {}
                            },
                            ClientState::Spectator => match requested_state {
                                // Become Registered first.
                                ClientState::Connected => {
                                    client.error_state(RequestStateError::Impossible)
                                }
                                ClientState::Spectator => {
                                    client.error_state(RequestStateError::Already)
                                }
                                ClientState::Registered
                                | ClientState::Character
                                | ClientState::Dead => client.allow_state(ClientState::Spectator),
                                ClientState::Pending => {}
                            },
                            // Use ClientMsg::Character instead.
                            ClientState::Character => {
                                client.error_state(RequestStateError::WrongMessage)
                            }
                            ClientState::Dead => client.error_state(RequestStateError::Impossible),
                            ClientState::Pending => {}
                        },
                        ClientMsg::Register { player } => match client.client_state {
                            ClientState::Connected => {
                                Self::initialize_player(state, entity, client, player);
                                if let Some(player) =
                                    state.ecs().read_storage::<comp::Player>().get(entity)
                                {
                                    new_chat_msgs
                                        .push((None, format!("{} logged in", &player.alias)));
                                }
                            }
                            // Use RequestState instead (No need to send `player` again).
                            _ => client.error_state(RequestStateError::Impossible),
                        },
                        ClientMsg::SetViewDistance(view_distance) => match client.client_state {
                            ClientState::Character { .. } => {
                                state
                                    .ecs_mut()
                                    .write_storage::<comp::Player>()
                                    .get_mut(entity)
                                    .map(|player| player.view_distance = Some(view_distance));
                            }
                            _ => {}
                        },
                        ClientMsg::Character { name, body } => match client.client_state {
                            // Become Registered first.
                            ClientState::Connected => {
                                client.error_state(RequestStateError::Impossible)
                            }
                            ClientState::Registered
                            | ClientState::Spectator
                            | ClientState::Dead => {
                                Self::create_player_character(state, entity, client, name, body)
                            }
                            ClientState::Character => {
                                client.error_state(RequestStateError::Already)
                            }
                            ClientState::Pending => {}
                        },
                        ClientMsg::Attack => match client.client_state {
                            ClientState::Character => {
                                if state
                                    .ecs()
                                    .read_storage::<comp::Attacking>()
                                    .get(entity)
                                    .is_none()
                                {
                                    state.write_component(entity, comp::Attacking::start());
                                }
                            }
                            _ => client.error_state(RequestStateError::Impossible),
                        },
                        ClientMsg::Respawn => match client.client_state {
                            ClientState::Dead => {
                                state.write_component(entity, comp::Respawning);
                            }
                            _ => client.error_state(RequestStateError::Impossible),
                        },
                        ClientMsg::Chat(msg) => match client.client_state {
                            ClientState::Connected => {
                                client.error_state(RequestStateError::Impossible)
                            }
                            ClientState::Registered
                            | ClientState::Spectator
                            | ClientState::Dead
                            | ClientState::Character => {
                                if msg.len() <= MAX_MSG_LEN {
                                    new_chat_msgs.push((Some(entity), msg))
                                }
                            }
                            ClientState::Pending => {}
                        },
                        ClientMsg::PlayerAnimation(animation_info) => {
                            match client.client_state {
                                ClientState::Character => {
                                    state.write_component(entity, animation_info)
                                }
                                // Only characters can send animations.
                                _ => client.error_state(RequestStateError::Impossible),
                            }
                        }
                        ClientMsg::PlayerPhysics { pos, vel, ori } => match client.client_state {
                            ClientState::Character => {
                                state.write_component(entity, pos);
                                state.write_component(entity, vel);
                                state.write_component(entity, ori);
                            }
                            // Only characters can send positions.
                            _ => client.error_state(RequestStateError::Impossible),
                        },
                        ClientMsg::TerrainChunkRequest { key } => match client.client_state {
                            ClientState::Connected
                            | ClientState::Registered
                            | ClientState::Dead => {
                                client.error_state(RequestStateError::Impossible);
                            }
                            ClientState::Spectator | ClientState::Character => {
                                match state.terrain().get_key(key) {
                                    Some(chunk) => {
                                        client.postbox.send_message(ServerMsg::TerrainChunkUpdate {
                                            key,
                                            chunk: Box::new(chunk.clone()),
                                        })
                                    }
                                    None => requested_chunks.push(key),
                                }
                            }
                            ClientState::Pending => {}
                        },
                        // Always possible.
                        ClientMsg::Ping => client.postbox.send_message(ServerMsg::Pong),
                        ClientMsg::Pong => {}
                        ClientMsg::Disconnect => {
                            disconnect = true;
                        }
                    }
                }
            } else if state.get_time() - client.last_ping > CLIENT_TIMEOUT || // Timeout
                client.postbox.error().is_some()
            // Postbox error
            {
                disconnect = true;
            } else if state.get_time() - client.last_ping > CLIENT_TIMEOUT * 0.5 {
                // Try pinging the client if the timeout is nearing.
                client.postbox.send_message(ServerMsg::Ping);
            }

            if disconnect {
                if let Some(player) = state.ecs().read_storage::<comp::Player>().get(entity) {
                    new_chat_msgs.push((None, format!("{} disconnected", &player.alias)));
                }
                disconnected_clients.push(entity);
                client.postbox.send_message(ServerMsg::Disconnect);
                true
            } else {
                false
            }
        });

        // Handle new chat messages.
        for (entity, msg) in new_chat_msgs {
            if let Some(entity) = entity {
                // Handle chat commands.
                if msg.starts_with("/") && msg.len() > 1 {
                    let argv = String::from(&msg[1..]);
                    self.process_chat_cmd(entity, argv);
                } else {
                    self.clients.notify_registered(ServerMsg::Chat(
                        match self.state.ecs().read_storage::<comp::Player>().get(entity) {
                            Some(player) => format!("[{}] {}", &player.alias, msg),
                            None => format!("[<anon>] {}", msg),
                        },
                    ));
                }
            } else {
                self.clients.notify_registered(ServerMsg::Chat(msg.clone()));
            }
            frontend_events.push(Event::Chat { entity, msg });
        }

        // Handle client disconnects.
        for entity in disconnected_clients {
            self.state.ecs_mut().delete_entity_synced(entity);

            frontend_events.push(Event::ClientDisconnected { entity });
        }

        // Generate requested chunks.
        for key in requested_chunks {
            self.generate_chunk(key);
        }

        Ok(frontend_events)
    }

    /// Initialize a new client states with important information.
    fn initialize_player(
        state: &mut State,
        entity: specs::Entity,
        client: &mut Client,
        player: comp::Player,
    ) {
        // Save player metadata (for example the username).
        state.write_component(entity, player);

        // Sync physics
        for (entity, &uid, &pos, &vel, &ori) in (
            &state.ecs().entities(),
            &state.ecs().read_storage::<Uid>(),
            &state.ecs().read_storage::<comp::phys::Pos>(),
            &state.ecs().read_storage::<comp::phys::Vel>(),
            &state.ecs().read_storage::<comp::phys::Ori>(),
        )
            .join()
        {
            client.notify(ServerMsg::EntityPhysics {
                entity: uid.into(),
                pos,
                vel,
                ori,
            });
        }

        // Sync animations
        for (entity, &uid, &animation_info) in (
            &state.ecs().entities(),
            &state.ecs().read_storage::<Uid>(),
            &state.ecs().read_storage::<comp::AnimationInfo>(),
        )
            .join()
        {
            client.notify(ServerMsg::EntityAnimation {
                entity: uid.into(),
                animation_info: animation_info.clone(),
            });
        }

        // Tell the client its request was successful.
        client.allow_state(ClientState::Registered);
    }

    /// Sync client states with the most up to date information.
    fn sync_clients(&mut self) {
        // Sync 'logical' state using Sphynx.
        self.clients
            .notify_registered(ServerMsg::EcsSync(self.state.ecs_mut().next_sync_package()));

        // Sync physics
        for (entity, &uid, &pos, &vel, &ori, force_update) in (
            &self.state.ecs().entities(),
            &self.state.ecs().read_storage::<Uid>(),
            &self.state.ecs().read_storage::<comp::phys::Pos>(),
            &self.state.ecs().read_storage::<comp::phys::Vel>(),
            &self.state.ecs().read_storage::<comp::phys::Ori>(),
            self.state
                .ecs()
                .read_storage::<comp::phys::ForceUpdate>()
                .maybe(),
        )
            .join()
        {
            let msg = ServerMsg::EntityPhysics {
                entity: uid.into(),
                pos,
                vel,
                ori,
            };

            let state = &self.state;
            let mut clients = &mut self.clients;

            let in_vd = |entity| {
                // Get client position.
                let client_pos = match state.ecs().read_storage::<comp::phys::Pos>().get(entity) {
                    Some(pos) => pos.0,
                    None => return false,
                };
                // Get client view distance
                let client_vd = match state.ecs().read_storage::<comp::Player>().get(entity) {
                    Some(comp::Player {
                        view_distance: Some(vd),
                        ..
                    }) => *vd,
                    _ => return false,
                };

                (pos.0 - client_pos)
                    .map2(TerrainChunkSize::SIZE, |d, sz| {
                        (d.abs() as u32) < client_vd * sz as u32
                    })
                    .reduce_and()
            };

            match force_update {
                Some(_) => clients.notify_ingame_if(msg, in_vd),
                None => clients.notify_ingame_if_except(entity, msg, in_vd),
            }
        }

        // Sync animations
        for (entity, &uid, &animation_info, force_update) in (
            &self.state.ecs().entities(),
            &self.state.ecs().read_storage::<Uid>(),
            &self.state.ecs().read_storage::<comp::AnimationInfo>(),
            self.state
                .ecs()
                .read_storage::<comp::phys::ForceUpdate>()
                .maybe(),
        )
            .join()
        {
            if animation_info.changed || force_update.is_some() {
                let msg = ServerMsg::EntityAnimation {
                    entity: uid.into(),
                    animation_info: animation_info.clone(),
                };
                match force_update {
                    Some(_) => self.clients.notify_ingame(msg),
                    None => self.clients.notify_ingame_except(entity, msg),
                }
            }
        }

        // Remove all force flags.
        self.state
            .ecs_mut()
            .write_storage::<comp::phys::ForceUpdate>()
            .clear();
    }

    pub fn generate_chunk(&mut self, key: Vec2<i32>) {
        if self.pending_chunks.insert(key) {
            let chunk_tx = self.chunk_tx.clone();
            let world = self.world.clone();
            self.thread_pool.execute(move || {
                let _ = chunk_tx.send((key, world.generate_chunk(key)));
            });
        }
    }

    fn process_chat_cmd(&mut self, entity: EcsEntity, cmd: String) {
        // Separate string into keyword and arguments.
        let sep = cmd.find(' ');
        let (kwd, args) = match sep {
            Some(i) => (cmd[..i].to_string(), cmd[(i + 1)..].to_string()),
            None => (cmd, "".to_string()),
        };

        // Find the command object and run its handler.
        let action_opt = CHAT_COMMANDS.iter().find(|x| x.keyword == kwd);
        match action_opt {
            Some(action) => action.execute(self, entity, args),
            // Unknown command
            None => {
                self.clients.notify(
                    entity,
                    ServerMsg::Chat(format!(
                        "Unrecognised command: '/{}'\ntype '/help' for a list of available commands",
                        kwd
                    )),
                );
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.clients.notify_registered(ServerMsg::Shutdown);
    }
}
