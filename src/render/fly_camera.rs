//! Minimal hand-rolled free-fly camera controller.

use bevy::app::AppExit;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, grab_cursor)
            .add_systems(Update, (exit_on_escape, look, movement));
    }
}

#[derive(Component)]
pub struct FlyCamera {
    pub speed: f32,
    pub sprint_multiplier: f32,
    pub sensitivity: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            speed: 4.0,
            sprint_multiplier: 4.0,
            sensitivity: 0.0025,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

impl FlyCamera {
    /// Builds a `FlyCamera` whose yaw/pitch match an existing rotation (e.g. one
    /// produced by `Transform::looking_at`), so the very first mouse-look update
    /// doesn't snap the view away from wherever the camera was initially pointed.
    pub fn from_rotation(rotation: Quat) -> Self {
        let (yaw, pitch, _roll) = rotation.to_euler(EulerRot::YXZ);
        Self {
            yaw,
            pitch,
            ..Default::default()
        }
    }
}

fn grab_cursor(mut cursor_options: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    let Ok(mut cursor_options) = cursor_options.single_mut() else {
        return;
    };
    cursor_options.grab_mode = CursorGrabMode::Locked;
    cursor_options.visible = false;
}

fn exit_on_escape(keyboard: Res<ButtonInput<KeyCode>>, mut app_exit: MessageWriter<AppExit>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
    }
}

fn look(mut mouse_motion: MessageReader<MouseMotion>, mut query: Query<(&mut FlyCamera, &mut Transform)>) {
    let mut delta = Vec2::ZERO;
    for motion in mouse_motion.read() {
        delta += motion.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }

    for (mut fly_camera, mut transform) in &mut query {
        fly_camera.yaw -= delta.x * fly_camera.sensitivity;
        fly_camera.pitch -= delta.y * fly_camera.sensitivity;
        fly_camera.pitch = fly_camera.pitch.clamp(-89f32.to_radians(), 89f32.to_radians());

        transform.rotation =
            Quat::from_axis_angle(Vec3::Y, fly_camera.yaw) * Quat::from_axis_angle(Vec3::X, fly_camera.pitch);
    }
}

fn movement(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&FlyCamera, &mut Transform)>,
) {
    for (fly_camera, mut transform) in &mut query {
        let mut direction = Vec3::ZERO;
        let forward = *transform.forward();
        let right = *transform.right();

        if keyboard.pressed(KeyCode::KeyW) {
            direction += forward;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            direction -= forward;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            direction += right;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            direction -= right;
        }
        if keyboard.pressed(KeyCode::Space) {
            direction += Vec3::Y;
        }
        if keyboard.pressed(KeyCode::ControlLeft) {
            direction -= Vec3::Y;
        }

        if direction == Vec3::ZERO {
            continue;
        }

        let mut speed = fly_camera.speed;
        if keyboard.pressed(KeyCode::ShiftLeft) {
            speed *= fly_camera.sprint_multiplier;
        }

        transform.translation += direction.normalize() * speed * time.delta_secs();
    }
}
