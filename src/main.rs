use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(140., 140.))),
        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.45, 0.2))),
    ));

    // Light
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_translation(Vec3::ONE * 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 30.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
