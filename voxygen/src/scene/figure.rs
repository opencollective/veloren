use crate::{
    anim::{
        character::{self, CharacterSkeleton},
        quadruped::{self, QuadrupedSkeleton},
        quadrupedmedium::{self, QuadrupedMediumSkeleton},
        Animation, Skeleton,
    },
    mesh::Meshable,
    render::{
        Consts, FigureBoneData, FigureLocals, FigurePipeline, Globals, Mesh, Model, Renderer,
    },
};
use client::Client;
use common::{
    assets,
    comp::{
        self,
        actor::{
            Belt, Chest, Draw, Foot, Hand, Head, Pants, PigChest, PigHead, PigLegL, PigLegR,
            Shoulder, Weapon, WolfEars, WolfFootLB, WolfFootLF, WolfFootRB, WolfFootRF,
            WolfHeadLower, WolfHeadUpper, WolfJaw, WolfTail, WolfTorsoBack, WolfTorsoMid,
        },
        Body,
    },
    figure::Segment,
    terrain::TerrainChunkSize,
    vol::VolSize,
};
use dot_vox::DotVoxData;
use log::warn;
use specs::{Entity as EcsEntity, Join};
use std::{collections::HashMap, f32};
use vek::*;

const DAMAGE_FADE_COEFFICIENT: f64 = 5.0;

pub struct FigureModelCache {
    models: HashMap<Body, (Model<FigurePipeline>, u64)>,
}

impl FigureModelCache {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
        }
    }

    pub fn get_or_create_model(
        &mut self,
        renderer: &mut Renderer,
        body: Body,
        tick: u64,
    ) -> &Model<FigurePipeline> {
        match self.models.get_mut(&body) {
            Some((_model, last_used)) => {
                *last_used = tick;
            }
            None => {
                self.models.insert(
                    body,
                    (
                        {
                            let bone_meshes = match body {
                                Body::Humanoid(body) => [
                                    Some(Self::load_head(body.head)),
                                    Some(Self::load_chest(body.chest)),
                                    Some(Self::load_belt(body.belt)),
                                    Some(Self::load_pants(body.pants)),
                                    Some(Self::load_left_hand(body.hand)),
                                    Some(Self::load_right_hand(body.hand)),
                                    Some(Self::load_left_foot(body.foot)),
                                    Some(Self::load_right_foot(body.foot)),
                                    Some(Self::load_weapon(body.weapon)),
                                    Some(Self::load_left_shoulder(body.shoulder)),
                                    Some(Self::load_right_shoulder(body.shoulder)),
                                    Some(Self::load_draw(body.draw)),
                                    Some(Self::load_left_equip(body.weapon)),
                                    Some(Self::load_right_equip(body.hand)),
                                    None,
                                    None,
                                ],
                                Body::Quadruped(body) => [
                                    Some(Self::load_pig_head(body.pig_head)),
                                    Some(Self::load_pig_chest(body.pig_chest)),
                                    Some(Self::load_pig_leg_lf(body.pig_leg_l)),
                                    Some(Self::load_pig_leg_rf(body.pig_leg_r)),
                                    Some(Self::load_pig_leg_lb(body.pig_leg_l)),
                                    Some(Self::load_pig_leg_rb(body.pig_leg_r)),
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                ],
                                Body::QuadrupedMedium(body) => [
                                    Some(Self::load_wolf_head_upper(body.wolf_head_upper)),
                                    Some(Self::load_wolf_jaw(body.wolf_jaw)),
                                    Some(Self::load_wolf_head_lower(body.wolf_head_lower)),
                                    Some(Self::load_wolf_tail(body.wolf_tail)),
                                    Some(Self::load_wolf_torso_back(body.wolf_torso_back)),
                                    Some(Self::load_wolf_torso_mid(body.wolf_torso_mid)),
                                    Some(Self::load_wolf_ears(body.wolf_ears)),
                                    Some(Self::load_wolf_foot_lf(body.wolf_foot_lf)),
                                    Some(Self::load_wolf_foot_rf(body.wolf_foot_rf)),
                                    Some(Self::load_wolf_foot_lb(body.wolf_foot_lb)),
                                    Some(Self::load_wolf_foot_rb(body.wolf_foot_rb)),
                                    None,
                                    None,
                                    None,
                                    None,
                                    None,
                                ],
                            };

                            let mut mesh = Mesh::new();
                            bone_meshes
                                .iter()
                                .enumerate()
                                .filter_map(|(i, bm)| bm.as_ref().map(|bm| (i, bm)))
                                .for_each(|(i, bone_mesh)| {
                                    mesh.push_mesh_map(bone_mesh, |vert| {
                                        vert.with_bone_idx(i as u8)
                                    })
                                });

                            renderer.create_model(&mesh).unwrap()
                        },
                        tick,
                    ),
                );
            }
        }

        &self.models[&body].0
    }

    pub fn clean(&mut self, tick: u64) {
        // TODO: Don't hard-code this.
        self.models
            .retain(|_, (_, last_used)| *last_used + 60 > tick);
    }

    // TODO: Don't make this public.
    pub fn load_mesh(filename: &str, position: Vec3<f32>) -> Mesh<FigurePipeline> {
        let full_path: String = ["voxygen/voxel/", filename].concat();
        Segment::from(assets::load_expect::<DotVoxData>(full_path.as_str()).as_ref())
            .generate_mesh(position)
    }

    fn load_head(head: Head) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match head {
                Head::Default => "figure/head.vox",
            },
            Vec3::new(-7.0, -5.5, -6.0),
        )
    }

    fn load_chest(chest: Chest) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match chest {
                Chest::Default => "figure/body/chest_male.vox",
                Chest::Blue => "armor/chest/chest_blue.vox",
                Chest::Brown => "armor/chest/chest_brown.vox",
                Chest::Dark => "armor/chest/chest_dark.vox",
                Chest::Green => "armor/chest/chest_green.vox",
                Chest::Orange => "armor/chest/chest_orange.vox",
            },
            Vec3::new(-6.0, -3.5, 0.0),
        )
    }

    fn load_belt(belt: Belt) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match belt {
                //Belt::Default => "figure/body/belt_male.vox",
                Belt::Dark => "armor/belt/belt_dark.vox",
            },
            Vec3::new(-5.0, -3.5, 0.0),
        )
    }

    fn load_pants(pants: Pants) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pants {
                Pants::Default => "figure/body/pants_male.vox",
                Pants::Blue => "armor/pants/pants_blue.vox",
                Pants::Brown => "armor/pants/pants_brown.vox",
                Pants::Dark => "armor/pants/pants_dark.vox",
                Pants::Green => "armor/pants/pants_green.vox",
                Pants::Orange => "armor/pants/pants_orange.vox",
            },
            Vec3::new(-5.0, -3.5, 0.0),
        )
    }

    fn load_left_hand(hand: Hand) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match hand {
                Hand::Default => "figure/body/hand.vox",
            },
            Vec3::new(-2.0, -2.5, -2.0),
        )
    }

    fn load_right_hand(hand: Hand) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match hand {
                Hand::Default => "figure/body/hand.vox",
            },
            Vec3::new(-2.0, -2.5, -2.0),
        )
    }

    fn load_left_foot(foot: Foot) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match foot {
                Foot::Default => "figure/body/foot.vox",
                Foot::Dark => "armor/foot/foot_dark.vox",
            },
            Vec3::new(-2.5, -3.5, -9.0),
        )
    }

    fn load_right_foot(foot: Foot) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match foot {
                Foot::Default => "figure/body/foot.vox",
                Foot::Dark => "armor/foot/foot_dark.vox",
            },
            Vec3::new(-2.5, -3.5, -9.0),
        )
    }

    fn load_weapon(weapon: Weapon) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match weapon {
                Weapon::Sword => "weapon/sword/sword_rusty_2h.vox",
                // TODO actually match against other weapons and set the right model
                _ => "weapon/sword/sword_rusty_2h.vox",
            },
            Vec3::new(-1.5, -6.5, -4.0),
        )
    }

    fn load_left_shoulder(shoulder: Shoulder) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match shoulder {
                Shoulder::Default => "armor/shoulder/shoulder_l_brown.vox",
            },
            Vec3::new(2.5, -0.5, 0.0),
        )
    }

    fn load_right_shoulder(shoulder: Shoulder) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match shoulder {
                Shoulder::Default => "armor/shoulder/shoulder_r_brown.vox",
            },
            Vec3::new(2.5, -0.5, 0.0),
        )
    }

    fn load_draw(draw: Draw) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match draw {
                Draw::Default => "object/glider.vox",
            },
            Vec3::new(-26.0, -26.0, -5.0),
        )
    }

    fn load_left_equip(weapon: Weapon) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match weapon {
                Weapon::Sword => "weapon/sword/sword_rusty_2h.vox",
                _ => "weapon/sword/sword_rusty_2h.vox",
            },
            Vec3::new(-1.5, -6.5, -4.5),
        )
    }

    fn load_right_equip(hand: Hand) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match hand {
                Hand::Default => "figure/body/hand.vox",
            },
            Vec3::new(-2.0, -2.5, -5.0),
        )
    }
    /////////
    fn load_pig_head(pig_head: PigHead) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pig_head {
                PigHead::Default => "npc/pig_purple/pig_head.vox",
            },
            Vec3::new(-6.0, 4.5, 3.0),
        )
    }

    fn load_pig_chest(pig_chest: PigChest) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pig_chest {
                PigChest::Default => "npc/pig_purple/pig_chest.vox",
            },
            Vec3::new(-5.0, 4.5, 0.0),
        )
    }

    fn load_pig_leg_lf(pig_leg_l: PigLegL) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pig_leg_l {
                PigLegL::Default => "npc/pig_purple/pig_leg_l.vox",
            },
            Vec3::new(0.0, -1.0, -1.5),
        )
    }

    fn load_pig_leg_rf(pig_leg_r: PigLegR) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pig_leg_r {
                PigLegR::Default => "npc/pig_purple/pig_leg_r.vox",
            },
            Vec3::new(0.0, -1.0, -1.5),
        )
    }

    fn load_pig_leg_lb(pig_leg_l: PigLegL) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pig_leg_l {
                PigLegL::Default => "npc/pig_purple/pig_leg_l.vox",
            },
            Vec3::new(0.0, -1.0, -1.5),
        )
    }

    fn load_pig_leg_rb(pig_leg_r: PigLegR) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match pig_leg_r {
                PigLegR::Default => "npc/pig_purple/pig_leg_r.vox",
            },
            Vec3::new(0.0, -1.0, -1.5),
        )
    }
    //////
    fn load_wolf_head_upper(wolf_upper_head: WolfHeadUpper) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_upper_head {
                WolfHeadUpper::Default => "npc/wolf/wolf_head_upper.vox",
            },
            Vec3::new(-7.0, -6.0, -5.5),
        )
    }

    fn load_wolf_jaw(wolf_jaw: WolfJaw) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_jaw {
                WolfJaw::Default => "npc/wolf/wolf_jaw.vox",
            },
            Vec3::new(-3.0, -3.0, -2.5),
        )
    }

    fn load_wolf_head_lower(wolf_head_lower: WolfHeadLower) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_head_lower {
                WolfHeadLower::Default => "npc/wolf/wolf_head_lower.vox",
            },
            Vec3::new(-7.0, -6.0, -5.5),
        )
    }

    fn load_wolf_tail(wolf_tail: WolfTail) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_tail {
                WolfTail::Default => "npc/wolf/wolf_tail.vox",
            },
            Vec3::new(-2.0, -12.0, -5.0),
        )
    }

    fn load_wolf_torso_back(wolf_torso_back: WolfTorsoBack) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_torso_back {
                WolfTorsoBack::Default => "npc/wolf/wolf_torso_back.vox",
            },
            Vec3::new(-7.0, -6.0, -6.0),
        )
    }

    fn load_wolf_torso_mid(wolf_torso_mid: WolfTorsoMid) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_torso_mid {
                WolfTorsoMid::Default => "npc/wolf/wolf_torso_mid.vox",
            },
            Vec3::new(-8.0, -5.5, -6.0),
        )
    }

    fn load_wolf_ears(wolf_ears: WolfEars) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_ears {
                WolfEars::Default => "npc/wolf/wolf_ears.vox",
            },
            Vec3::new(-4.0, -1.0, -1.0),
        )
    }

    fn load_wolf_foot_lf(wolf_foot_lf: WolfFootLF) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_foot_lf {
                WolfFootLF::Default => "npc/wolf/wolf_foot_lf.vox",
            },
            Vec3::new(-2.5, -4.0, -2.5),
        )
    }

    fn load_wolf_foot_rf(wolf_foot_rf: WolfFootRF) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_foot_rf {
                WolfFootRF::Default => "npc/wolf/wolf_foot_rf.vox",
            },
            Vec3::new(-2.5, -4.0, -2.5),
        )
    }

    fn load_wolf_foot_lb(wolf_foot_lb: WolfFootLB) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_foot_lb {
                WolfFootLB::Default => "npc/wolf/wolf_foot_lb.vox",
            },
            Vec3::new(-2.5, -4.0, -2.5),
        )
    }

    fn load_wolf_foot_rb(wolf_foot_rb: WolfFootRB) -> Mesh<FigurePipeline> {
        Self::load_mesh(
            match wolf_foot_rb {
                WolfFootRB::Default => "npc/wolf/wolf_foot_rb.vox",
            },
            Vec3::new(-2.5, -4.0, -2.5),
        )
    }
}

pub struct FigureMgr {
    model_cache: FigureModelCache,
    character_states: HashMap<EcsEntity, FigureState<CharacterSkeleton>>,
    quadruped_states: HashMap<EcsEntity, FigureState<QuadrupedSkeleton>>,
    quadruped_medium_states: HashMap<EcsEntity, FigureState<QuadrupedMediumSkeleton>>,
}

impl FigureMgr {
    pub fn new() -> Self {
        Self {
            model_cache: FigureModelCache::new(),
            character_states: HashMap::new(),
            quadruped_states: HashMap::new(),
            quadruped_medium_states: HashMap::new(),
        }
    }

    pub fn clean(&mut self, tick: u64) {
        self.model_cache.clean(tick);
    }

    pub fn maintain(&mut self, renderer: &mut Renderer, client: &Client) {
        let time = client.state().get_time();
        let ecs = client.state().ecs();
        let view_distance = client.view_distance().unwrap_or(1);
        // Get player position.
        let player_pos = client
            .state()
            .ecs()
            .read_storage::<comp::Pos>()
            .get(client.entity())
            .map_or(Vec3::zero(), |pos| pos.0);

        for (entity, pos, vel, ori, actor, animation_info, stats) in (
            &ecs.entities(),
            &ecs.read_storage::<comp::Pos>(),
            &ecs.read_storage::<comp::Vel>(),
            &ecs.read_storage::<comp::Ori>(),
            &ecs.read_storage::<comp::Actor>(),
            &ecs.read_storage::<comp::AnimationInfo>(),
            ecs.read_storage::<comp::Stats>().maybe(),
        )
            .join()
        {
            // Don't process figures outside the vd
            let vd_percent = (pos.0 - player_pos)
                .map2(TerrainChunkSize::SIZE, |d, sz| {
                    (100 * d.abs() as u32) / (view_distance * sz)
                })
                .reduce_max();
            // Keep from re-adding/removing entities on the border of the vd
            if vd_percent > 120 {
                match actor {
                    comp::Actor::Character { body, .. } => match body {
                        Body::Humanoid(_) => {
                            self.character_states.remove(&entity);
                        }
                        Body::Quadruped(_) => {
                            self.quadruped_states.remove(&entity);
                        }
                        Body::QuadrupedMedium(_) => {
                            self.quadruped_medium_states.remove(&entity);
                        }
                    },
                }
                continue;
            } else if vd_percent > 100 {
                continue;
            }

            // Change in health as color!
            let col = stats
                .and_then(|stats| stats.hp.last_change)
                .map(|(_, time, _)| {
                    Rgba::broadcast(1.0)
                        + Rgba::new(0.0, -1.0, -1.0, 0.0)
                            .map(|c| (c / (1.0 + DAMAGE_FADE_COEFFICIENT * time)) as f32)
                })
                .unwrap_or(Rgba::broadcast(1.0));

            match actor {
                comp::Actor::Character { body, .. } => match body {
                    Body::Humanoid(_) => {
                        let state = self.character_states.entry(entity).or_insert_with(|| {
                            FigureState::new(renderer, CharacterSkeleton::new())
                        });

                        let target_skeleton = match animation_info.animation {
                            comp::Animation::Idle => character::IdleAnimation::update_skeleton(
                                state.skeleton_mut(),
                                time,
                                animation_info.time,
                            ),
                            comp::Animation::Run => character::RunAnimation::update_skeleton(
                                state.skeleton_mut(),
                                (vel.linear.magnitude(), time),
                                animation_info.time,
                            ),
                            comp::Animation::Jump => character::JumpAnimation::update_skeleton(
                                state.skeleton_mut(),
                                time,
                                animation_info.time,
                            ),
                            comp::Animation::Attack => character::AttackAnimation::update_skeleton(
                                state.skeleton_mut(),
                                time,
                                animation_info.time,
                            ),
                            comp::Animation::Roll => character::RollAnimation::update_skeleton(
                                state.skeleton_mut(),
                                time,
                                animation_info.time,
                            ),
                            comp::Animation::Crun => character::CrunAnimation::update_skeleton(
                                state.skeleton_mut(),
                                (vel.linear.magnitude(), time),
                                animation_info.time,
                            ),
                            comp::Animation::Cidle => character::CidleAnimation::update_skeleton(
                                state.skeleton_mut(),
                                time,
                                animation_info.time,
                            ),
                            comp::Animation::Gliding => {
                                character::GlidingAnimation::update_skeleton(
                                    state.skeleton_mut(),
                                    (vel.linear.magnitude(), time),
                                    animation_info.time,
                                )
                            }
                        };

                        state.skeleton.interpolate(&target_skeleton);
                        state.update(renderer, pos.0, ori.0, col);
                    }
                    Body::Quadruped(_) => {
                        let state = self.quadruped_states.entry(entity).or_insert_with(|| {
                            FigureState::new(renderer, QuadrupedSkeleton::new())
                        });

                        let target_skeleton = match animation_info.animation {
                            comp::Animation::Run => quadruped::RunAnimation::update_skeleton(
                                state.skeleton_mut(),
                                (vel.linear.magnitude(), time),
                                animation_info.time,
                            ),
                            comp::Animation::Idle => quadruped::IdleAnimation::update_skeleton(
                                state.skeleton_mut(),
                                time,
                                animation_info.time,
                            ),
                            comp::Animation::Jump => quadruped::JumpAnimation::update_skeleton(
                                state.skeleton_mut(),
                                (vel.linear.magnitude(), time),
                                animation_info.time,
                            ),

                            // TODO!
                            _ => state.skeleton_mut().clone(),
                        };

                        state.skeleton.interpolate(&target_skeleton);
                        state.update(renderer, pos.0, ori.0, col);
                    }
                    Body::QuadrupedMedium(_) => {
                        let state =
                            self.quadruped_medium_states
                                .entry(entity)
                                .or_insert_with(|| {
                                    FigureState::new(renderer, QuadrupedMediumSkeleton::new())
                                });

                        let target_skeleton = match animation_info.animation {
                            comp::Animation::Run => quadrupedmedium::RunAnimation::update_skeleton(
                                state.skeleton_mut(),
                                (vel.linear.magnitude(), time),
                                animation_info.time,
                            ),
                            comp::Animation::Idle => {
                                quadrupedmedium::IdleAnimation::update_skeleton(
                                    state.skeleton_mut(),
                                    time,
                                    animation_info.time,
                                )
                            }
                            comp::Animation::Jump => {
                                quadrupedmedium::JumpAnimation::update_skeleton(
                                    state.skeleton_mut(),
                                    (vel.linear.magnitude(), time),
                                    animation_info.time,
                                )
                            }

                            // TODO!
                            _ => state.skeleton_mut().clone(),
                        };

                        state.skeleton.interpolate(&target_skeleton);
                        state.update(renderer, pos.0, ori.0, col);
                    }
                },
                // TODO: Non-character actors
            }
        }

        // Clear states that have dead entities.
        self.character_states
            .retain(|entity, _| ecs.entities().is_alive(*entity));
        self.quadruped_states
            .retain(|entity, _| ecs.entities().is_alive(*entity));
        self.quadruped_medium_states
            .retain(|entity, _| ecs.entities().is_alive(*entity));
    }

    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        client: &mut Client,
        globals: &Consts<Globals>,
    ) {
        let tick = client.get_tick();
        let ecs = client.state().ecs();

        let view_distance = client.view_distance().unwrap_or(1);
        // Get player position.
        let player_pos = client
            .state()
            .ecs()
            .read_storage::<comp::Pos>()
            .get(client.entity())
            .map_or(Vec3::zero(), |pos| pos.0);

        for (entity, _, _, _, actor, _, _) in (
            &ecs.entities(),
            &ecs.read_storage::<comp::Pos>(),
            &ecs.read_storage::<comp::Vel>(),
            &ecs.read_storage::<comp::Ori>(),
            &ecs.read_storage::<comp::Actor>(),
            &ecs.read_storage::<comp::AnimationInfo>(),
            ecs.read_storage::<comp::Stats>().maybe(),
        )
            .join()
            // Don't render figures outside the vd
            .filter(|(_, pos, _, _, _, _, _)| {
                (pos.0 - player_pos)
                    .map2(TerrainChunkSize::SIZE, |d, sz| {
                        (d.abs() as u32) < view_distance * sz as u32
                    })
                    .reduce_and()
            })
            // Don't render dead entities
            .filter(|(_, _, _, _, _, _, stats)| stats.map_or(true, |s| !s.is_dead))
        {
            match actor {
                comp::Actor::Character { body, .. } => {
                    if let Some((locals, bone_consts)) = match body {
                        Body::Humanoid(_) => self
                            .character_states
                            .get(&entity)
                            .map(|state| (state.locals(), state.bone_consts())),
                        Body::Quadruped(_) => self
                            .quadruped_states
                            .get(&entity)
                            .map(|state| (state.locals(), state.bone_consts())),
                        Body::QuadrupedMedium(_) => self
                            .quadruped_medium_states
                            .get(&entity)
                            .map(|state| (state.locals(), state.bone_consts())),
                    } {
                        let model = self.model_cache.get_or_create_model(renderer, *body, tick);

                        renderer.render_figure(model, globals, locals, bone_consts);
                    } else {
                        warn!("Body has no saved figure");
                    }
                }
            }
        }
    }
}

pub struct FigureState<S: Skeleton> {
    bone_consts: Consts<FigureBoneData>,
    locals: Consts<FigureLocals>,
    skeleton: S,
}

impl<S: Skeleton> FigureState<S> {
    pub fn new(renderer: &mut Renderer, skeleton: S) -> Self {
        Self {
            bone_consts: renderer
                .create_consts(&skeleton.compute_matrices())
                .unwrap(),
            locals: renderer.create_consts(&[FigureLocals::default()]).unwrap(),
            skeleton,
        }
    }

    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        pos: Vec3<f32>,
        ori: Vec3<f32>,
        col: Rgba<f32>,
    ) {
        let mat = Mat4::<f32>::identity()
            * Mat4::translation_3d(pos)
            * Mat4::rotation_z(-ori.x.atan2(ori.y))
            * Mat4::scaling_3d(Vec3::from(0.8));

        let locals = FigureLocals::new(mat, col);
        renderer.update_consts(&mut self.locals, &[locals]).unwrap();

        renderer
            .update_consts(&mut self.bone_consts, &self.skeleton.compute_matrices())
            .unwrap();
    }

    pub fn locals(&self) -> &Consts<FigureLocals> {
        &self.locals
    }

    pub fn bone_consts(&self) -> &Consts<FigureBoneData> {
        &self.bone_consts
    }

    pub fn skeleton_mut(&mut self) -> &mut S {
        &mut self.skeleton
    }
}
