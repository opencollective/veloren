use crate::{
    comp::{
        Acceleration, Gliding, Jumping, MoveDir, OnGround, Ori, Pos, Position, Rolling, Stats, Vel,
        Velocity,
    },
    state::DeltaTime,
    terrain::TerrainMap,
    vol::{ReadVol, Vox},
};
use specs::{Entities, Join, Read, ReadExpect, ReadStorage, System, WriteStorage};
use vek::*;

const GRAV_ACCEL: f32 = 9.81 * 4.0;
const FRIC_GROUND: f32 = 0.15;
const FRIC_AIR: f32 = 0.015;
const HUMANOID_ACCEL: f32 = 70.0;
const HUMANOID_SPEED: f32 = 120.0;
const HUMANOID_AIR_ACCEL: f32 = 10.0;
const HUMANOID_AIR_SPEED: f32 = 100.0;
const HUMANOID_JUMP_ACCEL: f32 = 16.0;
const ROLL_ACCEL: f32 = 160.0;
const ROLL_SPEED: f32 = 550.0;
const GLIDE_ACCEL: f32 = 15.0;
const GLIDE_SPEED: f32 = 45.0;
// Gravity is 9.81 * 4, so this makes gravity equal to .15
const GLIDE_ANTIGRAV: f32 = 9.81 * 3.95;

//// Integrates forces, calculates the new velocity based off of the old velocity
//// dt = delta time
//// lv = linear velocity
//// damp = linear damping
//// Friction is a type of damping.
//fn integrate_forces(dt: f32, mut lv: Vec3<f32>, damp: f32) -> Vec3<f32> {
//    lv.z -= (GRAVITATIONAL_ACCEL * dt).max(-50.0);
//
//    let mut linear_damp = 1.0 - dt * damp;
//
//    if linear_damp < 0.0
//    // reached zero in the given time
//    {
//        linear_damp = 0.0;
//    }
//
//    lv *= linear_damp;
//
//    lv
//}

/// Handles gravity, ground friction, air resistance, etc.
fn resolve_forces(lin_vel: Velocity, on_ground: bool) -> Acceleration {
    let gravity: Acceleration = Acceleration::new(0.0, 0.0, get_grav_accel(on_ground));

    let speed_squared = lin_vel.magnitude_squared();
    let mut friction: Acceleration = if on_ground {
        Acceleration::new(1.0, 1.0, 0.0)
    } else {
        Acceleration::broadcast(1.0)
    };
    friction *= 0.5 * get_friction_factor(on_ground) * speed_squared;

    gravity - friction
}

/// Gets the appropriate gravitational acceleration.
fn get_grav_accel(on_ground: bool) -> f32 {
    if on_ground {
        0.0
    } else {
        -GRAV_ACCEL
    }
}

/// Gets the appropriate friction factor.
fn get_friction_factor(on_ground: bool) -> f32 {
    // TODO: Determine ground friction by block type (use enum)
    50.0 * if on_ground { FRIC_GROUND } else { FRIC_AIR }
}

/// This system applies forces and calculates new positions and velocities.
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, TerrainMap>,
        Read<'a, DeltaTime>,
        ReadStorage<'a, MoveDir>,
        ReadStorage<'a, Gliding>,
        ReadStorage<'a, Stats>,
        WriteStorage<'a, Jumping>,
        WriteStorage<'a, Rolling>,
        WriteStorage<'a, OnGround>,
        WriteStorage<'a, Pos>,
        WriteStorage<'a, Vel>,
        WriteStorage<'a, Ori>,
    );

    fn run(
        &mut self,
        (
            entities,
            terrain,
            dt,
            move_dirs,
            glidings,
            stats,
            mut jumpings,
            mut rollings,
            mut on_grounds,
            mut positions,
            mut velocities,
            mut orientations,
        ): Self::SystemData,
    ) {
        // Apply movement inputs
        for (entity, stats, move_dir, gliding, mut pos, mut vel, mut ori) in (
            &entities,
            &stats,
            move_dirs.maybe(),
            glidings.maybe(),
            &mut positions,
            &mut velocities,
            &mut orientations,
        )
            .join()
        {
            // Disable while dead TODO: Replace with client states?
            if stats.is_dead {
                continue;
            }

            let on_ground = on_grounds.get(entity).is_some();

            // Move player according to move_dir
            if let Some(move_dir) = move_dir {
                vel.linear += Vec2::broadcast(dt.0)
                    * move_dir.0
                    * match (on_ground, gliding.is_some(), rollings.get(entity).is_some()) {
                        (true, false, false) if vel.linear.magnitude() < HUMANOID_SPEED => {
                            HUMANOID_ACCEL
                        }
                        (false, true, false) if vel.linear.magnitude() < GLIDE_SPEED => GLIDE_ACCEL,
                        (false, false, false) if vel.linear.magnitude() < HUMANOID_AIR_SPEED => {
                            HUMANOID_AIR_ACCEL
                        }
                        (true, false, true) if vel.linear.magnitude() < ROLL_SPEED => ROLL_ACCEL,

                        _ => 0.0,
                    };
            }

            // Jump
            if jumpings.get(entity).is_some() {
                vel.linear.z = HUMANOID_JUMP_ACCEL;
                jumpings.remove(entity);
            }

            // Glide
            if gliding.is_some() && vel.linear.magnitude() < GLIDE_SPEED && vel.linear.z < 0.0 {
                let lift = GLIDE_ANTIGRAV + vel.linear.z.powf(2.0) * 0.2;
                vel.linear.z +=
                    dt.0 * lift * Vec2::<f32>::from(vel.linear * 0.15).magnitude().min(1.0);
            }

            // Roll
            if let Some(time) = rollings.get_mut(entity).map(|r| &mut r.time) {
                *time += dt.0;
                if *time > 0.55 {
                    rollings.remove(entity);
                }
            }

            // Velocity Verlet --------
            // Determines position and velocity based on the previous velocity and position.
            // This algorithm is not as cheap as standard Verlet or Euler but is far more accurate.
            // accounting for all forces applied over time `dt`. // TODO: Also the entity's mass.
            // Performing these half time-step calculations allows for accurate calculations with
            // velocity- or position-based accelerations. If this step is omitted, the results will
            // match this more complete algorithm iff accelerations are solely dependent on time.
            println!(
                "Before calcs ------\npos: {:?}\nvel: {:?}\naccel: {:?}\ndt:{}\n------",
                pos.0, vel.linear, vel.accel, dt.0
            );
            let half_dt = 0.5 * dt.0;
            println!("Half dt: {}", half_dt);
            let mut half_accel = vel.accel;
            half_accel *= half_dt;
            let half_step_vel: Velocity = vel.linear + half_accel;
            pos.0 += half_step_vel * dt.0;
            println!("Half-vel: {:?}\nUpdated pos: {:?}", half_step_vel, pos.0);
            // TODO: Resolve collisions, change accelerations/velocities accordingly.
            // Update entity's velocity and acceleration.
            let new_accel: Acceleration = resolve_forces(vel.linear, on_ground);
            println!("New accel: {:?}", new_accel);
            let mut combined_accel = vel.accel + new_accel;
            println!("Combined accel: {:?}", combined_accel);
            combined_accel *= half_dt;
            println!("Times half dt: {:?}", combined_accel);
            vel.linear = half_step_vel + combined_accel;
            println!("New vel: {:?}", vel.linear);
            if vel.linear.z > HUMANOID_AIR_SPEED {
                vel.linear.z = HUMANOID_AIR_SPEED;
            } else if vel.linear.z < -HUMANOID_AIR_SPEED {
                vel.linear.z = -HUMANOID_AIR_SPEED;
            }
            println!("New vel(z): {}", vel.linear.z);
            vel.accel = new_accel;
            // ------------------------

            // Set orientation based on velocity
            if vel.linear.magnitude_squared() != 0.0 {
                ori.0 = vel.linear.normalized() * Vec3::new(1.0, 1.0, 0.0);
            }

            // Update OnGround component
            if terrain
                .get((pos.0 - Vec3::unit_z() * 0.1).map(|e| e.floor() as i32))
                .map(|vox| !vox.is_empty())
                .unwrap_or(false)
                && vel.linear.z <= 0.0
            {
                on_grounds.insert(entity, OnGround);
            } else {
                on_grounds.remove(entity);
            }

            // Basic collision with terrain
            let mut i = 0.0;
            while terrain
                .get(pos.0.map(|e| e.floor() as i32))
                .map(|vox| !vox.is_empty())
                .unwrap_or(false)
                && i < 6000.0 * dt.0
            {
                pos.0.z += 0.0025;
                vel.linear.z = 0.0;
                i += 1.0;
            }
        }
    }
}
