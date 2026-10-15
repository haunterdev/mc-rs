//! Posing other players each frame: `RenderLivingBase.doRender`'s body transform, the part
//! angles from `ModelBiped.setRotationAngles`, and the system that writes them.

use super::*;

/// `RenderLivingBase.doRender`'s transform down to the model's space, before the model's 1/16
/// units: the interpolated feet (0.125 lower sneaking, `RenderPlayer.doRender`), rotate
/// 180 - renderYawOffset, scale (-1, -1, 1), the player's 0.9375, translate down 1.501.
pub(super) fn body_matrix(o: &Other, partial: f32) -> Mat4 {
    let mut pos = o.prev_pos.lerp(o.pos, partial as f64);
    if o.sneaking() {
        pos.y -= 0.125;
    }
    let body_yaw = lerp(o.prev_body_yaw, o.body_yaw, partial);
    Mat4::from_translation(pos.as_vec3())
        * Mat4::from_rotation_y((180.0 - body_yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-0.9375, -0.9375, 0.9375))
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0))
}

/// Part pivots and angles from `ModelBiped.setRotationAngles` (the player model's own overrides
/// change nothing for an empty or item-holding hand).
pub(super) fn pose(o: &Other, partial: f32) -> [(Vec3, Vec3); 6] {
    let limb_amount = lerp(o.prev_limb_amount, o.limb_amount, partial).min(1.0);
    let limb_swing = o.limb_swing - o.limb_amount * (1.0 - partial);
    let age = o.age as f32 + partial;
    let body_yaw = lerp(o.prev_body_yaw, o.body_yaw, partial);
    let yaw = o.prev_yaw + wrap(o.yaw - o.prev_yaw) * partial;
    let head_yaw = wrap(yaw - body_yaw).to_radians();
    let head_pitch = lerp(o.prev_pitch, o.pitch, partial).to_radians();
    let sneak = o.sneaking();
    let swing = o.swing_progress(partial);

    let mut head = (Vec3::ZERO, Vec3::new(head_pitch, head_yaw, 0.0));
    let mut body = (Vec3::ZERO, Vec3::ZERO);
    let mut right_arm = (Vec3::new(-5.0, 2.0, 0.0), Vec3::ZERO);
    let mut left_arm = (Vec3::new(5.0, 2.0, 0.0), Vec3::ZERO);
    let mut right_leg = (Vec3::new(-1.9, 12.0, 0.1), Vec3::ZERO);
    let mut left_leg = (Vec3::new(1.9, 12.0, 0.1), Vec3::ZERO);

    right_arm.1.x = (limb_swing * 0.6662 + PI).cos() * 2.0 * limb_amount * 0.5;
    left_arm.1.x = (limb_swing * 0.6662).cos() * 2.0 * limb_amount * 0.5;
    right_leg.1.x = (limb_swing * 0.6662).cos() * 1.4 * limb_amount;
    left_leg.1.x = (limb_swing * 0.6662 + PI).cos() * 1.4 * limb_amount;
    // ArmPose.ITEM for anything in the main hand (RenderPlayer.setModelVisibilities).
    if o.target.held != 0 || o.target.held_item != 0 {
        right_arm.1.x = right_arm.1.x * 0.5 - PI / 10.0;
    }
    // ArmPose.ITEM for anything in the off hand.
    if o.target.held_item_off != 0 {
        left_arm.1.x = left_arm.1.x * 0.5 - PI / 10.0;
    }
    if swing > 0.0 {
        body.1.y = ((swing.sqrt()) * PI * 2.0).sin() * 0.2;
        right_arm.0.z = body.1.y.sin() * 5.0;
        right_arm.0.x = -body.1.y.cos() * 5.0;
        left_arm.0.z = -body.1.y.sin() * 5.0;
        left_arm.0.x = body.1.y.cos() * 5.0;
        right_arm.1.y += body.1.y;
        left_arm.1.y += body.1.y;
        left_arm.1.x += body.1.y;
        let mut f1 = 1.0 - swing;
        f1 *= f1;
        f1 *= f1;
        f1 = 1.0 - f1;
        let f2 = (f1 * PI).sin();
        let f3 = (swing * PI).sin() * -(head.1.x - 0.7) * 0.75;
        right_arm.1.x -= f2 * 1.2 + f3;
        right_arm.1.y += body.1.y * 2.0;
        right_arm.1.z += (swing * PI).sin() * -0.4;
    }
    if sneak {
        body.1.x = 0.5;
        right_arm.1.x += 0.4;
        left_arm.1.x += 0.4;
        right_leg.0.z = 4.0;
        left_leg.0.z = 4.0;
        right_leg.0.y = 9.0;
        left_leg.0.y = 9.0;
        head.0.y = 1.0;
    }
    right_arm.1.z += (age * 0.09).cos() * 0.05 + 0.05;
    left_arm.1.z -= (age * 0.09).cos() * 0.05 + 0.05;
    right_arm.1.x += (age * 0.067).sin() * 0.05;
    left_arm.1.x -= (age * 0.067).sin() * 0.05;
    [head, body, right_arm, left_arm, right_leg, left_leg]
}

/// Places every player model and name tag for this frame.
#[allow(clippy::too_many_arguments)]
pub fn pose_others(
    clock: Res<Clock>,
    view: Res<View>,
    hide: Res<HideGui>,
    streamer: Res<Streamer>,
    plate_mats: Option<Res<PlateMaterials>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    players: Query<(Entity, &Other, Has<OwnModel>)>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut plate_handles: Query<&mut MeshMaterial3d<NameplateMaterial>>,
) {
    let partial = clock.partial;
    let set_vis = |visibility: &mut Query<&mut Visibility>, e: Entity, on: bool| {
        if let Ok(mut v) = visibility.get_mut(e) {
            let want = if on { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
    };
    // RenderManager.playerViewY and playerViewX, turned round for the front view.
    let (view_yaw, view_pitch) = if view.third_person == 2 { (view.yaw + 180.0, -view.pitch) } else { (view.yaw, view.pitch) };
    for (e, o, own) in &players {
        let shown = o.increments >= 0 && (!own || view.third_person != 0);
        set_vis(&mut visibility, e, shown);
        if !shown {
            if let Some(p) = o.plate {
                for piece in [p.bg, p.through, p.text] {
                    set_vis(&mut visibility, piece, false);
                }
            }
            continue;
        }
        let mut pos = o.prev_pos.lerp(o.pos, partial as f64);
        if o.sneaking() {
            pos.y -= 0.125;
        }
        // Then the model's 1/16 units.
        let root = body_matrix(o, partial) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        if let Ok(mut t) = transforms.get_mut(e) {
            *t = Transform::from_matrix(root);
        }
        for (i, (pivot, angles)) in pose(o, partial).into_iter().enumerate() {
            if let Ok(mut t) = transforms.get_mut(o.parts[i]) {
                // ModelRenderer.render: translate to the pivot, rotate Z, Y, X.
                *t = Transform { translation: pivot, rotation: Quat::from_euler(EulerRot::ZYX, angles.z, angles.y, angles.x), scale: Vec3::ONE };
            }
        }
        // getBrightnessForRender at the eyes: the lightmap coordinates the shader looks up.
        let eye = pos + DVec3::new(0.0, 1.62, 0.0);
        let (sky, block) = neighbor_light(&streamer, eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        let light = Vec4::new(block as f32 * 16.0, sky as f32 * 16.0, 0.0, 0.0);
        if materials.get(&o.material).is_some_and(|m| m.light != light) {
            if let Some(mut m) = materials.get_mut(&o.material) {
                m.light = light;
            }
        }

        // Render.renderLivingLabel: within 64 blocks of the viewer (32 sneaking), with the GUI.
        let Some(p) = o.plate else { continue };
        let Some(pm) = plate_mats.as_deref() else { continue };
        let sneak = o.sneaking();
        let range = if sneak { 32.0 } else { 64.0 };
        let near = (o.pos - view.feet.as_dvec3()).length_squared() <= range * range;
        let on = near && !hide.0;
        set_vis(&mut visibility, p.bg, on);
        set_vis(&mut visibility, p.through, on && !sneak);
        set_vis(&mut visibility, p.text, on);
        if !on {
            continue;
        }
        // drawNameplate: over the head by height + 0.5 (0.25 less sneaking), turned to the
        // viewer, scaled to 0.025 a font pixel with y down.
        let at = pos.as_vec3() + Vec3::Y * (1.8 + 0.5 - if sneak { 0.25 } else { 0.0 });
        let m = Mat4::from_translation(at)
            * Mat4::from_rotation_y((-view_yaw).to_radians())
            * Mat4::from_rotation_x(view_pitch.to_radians())
            * Mat4::from_scale(Vec3::new(-0.025, -0.025, 0.025));
        // The background sorts behind the text in the blended pass: it draws first, as vanilla
        // draws it first.
        let away = (at - view.eye).normalize_or_zero() * 0.05;
        for (piece, offset) in [(p.bg, away), (p.through, Vec3::ZERO), (p.text, Vec3::ZERO)] {
            if let Ok(mut t) = transforms.get_mut(piece) {
                *t = Transform::from_matrix(Mat4::from_translation(offset) * m);
            }
        }
        for (piece, want) in [(p.bg, if sneak { &pm.bg_depth } else { &pm.bg }), (p.text, if sneak { &pm.text_dim } else { &pm.text })] {
            if let Ok(mut h) = plate_handles.get_mut(piece) {
                if h.0 != *want {
                    h.0 = want.clone();
                }
            }
        }
    }
}
