//! Alle Funktionen, die benötigt werden, um ein Fenster zu öffnen und
//! ein Bild darin parallel zum Erdboden ausgerichtet zu halten.

use std::{cell::Cell, f32::consts::PI, path::Path, rc::Rc};

use anyhow::{Result, anyhow};
use image::{ImageBuffer, Rgba};
use macroquad::prelude::*;
use miniquad::window;

use crate::{monitor::{OrientationVectors, Rotation}, serial::SerialReader};


/// Öffnet das Fenster und lädt das Bild von der Datei in den Arbeitsspeicher.
/// Der Rendervorgang ist vollständig in [`run_window_loop`] implementiert.
pub fn run_image_stabilizer(
    fullscreen: bool,
    background_color: Color,
    image_path: &Path,
    orientations: OrientationVectors,
    serial_reader: SerialReader
) -> Result<()> {
    // Konfiguration des Fensters.
    let config = Conf {
        fullscreen,
        icon: None,

        ..Default::default()
    };

    // Lese und dekodiere das Bild
    let loaded_img = image::open(image_path)?;
    let rgb8a_img = loaded_img.into_rgba8();

    // `from_config` erlaubt es nicht, dass die übergebene Funktion einen Rückgabewert hat.
    // Deshalb wird ein Workaround genutzt, um mögliche Fehler dennoch erfassen zu können:
    let error = Rc::new(Cell::new(None));

    // erstelle das Fenster und starte den Render-loop
    macroquad::Window::from_config(
        config,
        run_window_loop(
            background_color,
            rgb8a_img,
            orientations,
            serial_reader,
            error.clone()
        )
    );

    // Überprüfe, ob ein Fehler aufgetreten ist.
    match error.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Führt den Renderloop aus.
/// Die Funktion muss als `async` markiert sein und darf keinen Rückgabewert haben.
async fn run_window_loop(
    background_color: Color,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    orientations: OrientationVectors,
    mut serial_reader: SerialReader,
    error: Rc<Cell<Option<anyhow::Error>>>
) {
    // Lade das Bild als GPU Textur in den VRAM.
    let image_texture = Texture2D::from_rgba8(
        image.width() as u16,
        image.height() as u16,
        &image
    );

    // Nachdem die Textur geladen wurde, kann das Bild aus dem Arbeitsspeicher entfernt werden.
    drop(image);

    let image_size = image_texture.size();

    let mut window_size = Vec2::ZERO;
    let mut image_origin = Vec2::ZERO;
    let mut scaled_image_size = Vec2::ZERO;

    let mut rotation_paused = false;
    let mut angle: f32 = 0.0;

    loop {
        // Lese alle Tastaturereignisse und pausiere die Rotation, wenn die Leertaste gedrückt wurde.
        loop {
            match macroquad::input::get_char_pressed() {
                Some(' ') => rotation_paused = !rotation_paused,
                Some(_) => continue,
                None => break
            }
        }

        // Warte auf den nächsten Beschleunigungswert und berechne den Winkel.
        let new_angle = match serial_reader.next() {
            Some(Ok(acceleration)) => angle_from_vec(acceleration, &orientations),
            Some(Err(e)) => {
                error.set(Some(e));
                return;
            }
            None => {
                error.set(Some(anyhow!("serielle Verbindung geschlossen")));
                return;
            }
        };

        // Fenstergröße muss jeden Frame erneut eingelesen werden, da es möglich ist, dass das Fenster vergrößert / verkleinert wurde.
        let new_window_size = Vec2::from_array(window::screen_size().into());

        // Aktualisiere, wenn ein neuer Winkel vorliegt (und die Rotation nicht pausiert ist) oder sich die Fenstergröße geändert hat.
        if new_window_size != window_size || (!rotation_paused && new_angle != angle) {
            // aktualisiere Winkel und Fenstergröße.
            angle = new_angle;
            window_size = new_window_size;

            // Berechne die neue Bildskalierung.
            let window_corner_angles = get_corner_angles(window_size);
            let image_corner_angles = get_corner_angles(image_size);
            let window_midpoint = window_size / 2.0;

            // Bestimme die kleinere der beiden Bilddiagonalen.
            let smallest_len = image_corner_angles
                .into_iter()
                .take(2)
                .map(|corner| {
                    // addiere den Winkel der Bildecke und die Drehung des Bildschirms
                    // `angle` und `corner` sind nicht negativ
                    // => durch Modulo 2π wird sichergestellt, dass Winkel zwischen 0 und 2π liegt.
                    let actual_angle = (angle + corner).rem_euclid(2.0*PI);

                    let (a, beta) = match window_corner_angles.binary_search_by(|f| f.total_cmp(&actual_angle)) {
                        // Fensterecke berührt eine Bildecke:
                        // Diagonale des Bildes entspricht der Diagonale des Fensters.
                        Ok(_) => return window_midpoint.length(),

                        // Die Diagonale liegt auf einer Kante.
                        // Achtung: 0 und 4 bedeuten beide, dass die obere Kante berührt wird,
                        // allerdings steht 0 für einen spitzen und 4 für einen überstumpfen Winkel.
                        Err(0) => (window_midpoint.y, actual_angle),
                        Err(1) => (window_midpoint.x, (actual_angle - 0.5*PI).abs()),
                        Err(2) => (window_midpoint.y, (actual_angle - PI).abs()),
                        Err(3) => (window_midpoint.x, (actual_angle - 1.5*PI).abs()),
                        Err(4) => (window_midpoint.y, (actual_angle - 2.0*PI).abs()),

                        _ => unreachable!("window_corner_angles sollte nicht mehr als vier Elemente enthalten")
                    };

                    // Länge der Diagonalen.
                    a / beta.cos()
                })
                .min_by(f32::total_cmp)
                .unwrap();

            // Skalierung und abgeleitete Werte können mithilfe der skalierten Bilddiagonalen berechnet werden.
            let scale = smallest_len / (image_size.length() * 0.5);
            scaled_image_size = image_size * scale;
            image_origin = window_midpoint - (scaled_image_size / 2.0);
        }


        // fülle den Hintergrund hinter dem Bild mit einer einheitlichen Farbe.
        clear_background(background_color);

        // Zeichne das Bild.
        draw_texture_ex(
            &image_texture,
            image_origin.x, image_origin.y,
            WHITE, // Die Farbe hat bei nichttransparenten Bildern keinen Einfluss, muss aber dennoch angegeben werden.
            DrawTextureParams {
                dest_size: Some(scaled_image_size), // Größe des Bildes als Vektor.
                rotation: angle, // Rotation im Bogenmaß.

                ..Default::default()
            },
        );

        // Warte auf den nächsten Frame und beginne von vorn.
        next_frame().await;
    }
}


/// Nutzt die Richtungsvektoren und den Beschleunigungswert, um die Rotation des Monitors zu bestimmen.
fn angle_from_vec(vec: Vec3, orientations: &OrientationVectors) -> f32 {
    // Richtungsvektoren der Ausrichtungen `none` und `right`.
    let orientations_none = orientations.0[&Rotation::None];
    let orientations_right = orientations.0[&Rotation::Right];

    // Projiziere den Beschleunigungsvektor auf die Rotationsebene.
    let parallel_none = vec.project_onto(orientations_none);
    let parallel_right = vec.project_onto(orientations_right);
    let projected_vec = parallel_none + parallel_right;

    // Berechne die Winkel zwischen Richtungsvektoren und dem projiziertem Beschleunigungsvektor.
    let abs_angle = projected_vec.angle_between(orientations_none);
    let angle_r = projected_vec.angle_between(orientations_right);

    // Wenn `angle_r` < π/2 ist, zeigt der Winkel bereits in die richtige Richtung,
    // andernfalls wird der korrekte Winkel durch 2π - `abs_angle` erhalten.
    if angle_r < 0.5*PI {
        abs_angle
    } else {
        2.0*PI - abs_angle
    }
}

/// Berechnet die Position der Ecken im Gradmaß anhand der Größe des gegebenen Rechtecks.
fn get_corner_angles(size: Vec2) -> [f32; 4] {
    let midpoint = size / 2.0;

    // Innerhalb eines Rechtecks gibt es zwei verschiedene Winkel zwischen der Senkrechten durch den Mittelpunkt und den Ecken.
    let inner_angle_b = (midpoint.y / midpoint.x).atan();
    let inner_angle_a = (midpoint.x / midpoint.y).atan();

    // Winkel aller Ecken, gemessen relativ zur Senkrechten durch den Mittelpunkt und die obere Kante.
    [inner_angle_a, 0.5*PI + inner_angle_b, PI + inner_angle_a, 1.5*PI + inner_angle_b]
}
