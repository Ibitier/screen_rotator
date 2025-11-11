//! Enthält alle Methoden, die zur Auflistung und Rotation des Bildschirms unter KDE Plasma benötigt werden,
//! sowie das [`OrientationVectors`]-Struct, das die Richtungsvektoren repräsentiert.

use std::{collections::BTreeMap, fmt::Display, process::{Command, ExitStatus, Stdio}};

use anyhow::{anyhow, Result};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::{serial::SerialReader};


/// Auflistung aller Rotationen, die `kscreen-doctor` unterstützt.
#[derive(PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Clone, Copy)]
#[repr(u8)]
pub enum Rotation {
    None = 0,
    Right = 1,
    Inverted = 2,
    Left = 3,
}

impl Rotation {
    /// Gibt die Rotation im von `kscreen-doctor` erwarteten Format zurück.
    fn to_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Left => "left",
            Self::Right => "right",
            Self::Inverted => "inverted",
        }
    }
}

/// Ermöglicht die Nutzung von [`Display`] im [`format!`]-Macro.
impl Display for Rotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}


/// Repräsentiert einen Monitor, wie ihn `kscreen-doctor` zurückliefert.
/// Implementiert [`Deserialize`], da dieses Struct einem Eintrag im `outputs`-Array der `kscreen-doctor`-Ausgabe entspricht.
#[derive(Serialize, Deserialize, Clone)]
pub struct PlasmaMonitor {
    pub name: String,
}

/// Wird von [`select_monitor`](crate::args::Args::select_monitor) benötigt.
#[allow(clippy::to_string_trait_impl)]
impl ToString for PlasmaMonitor {
    fn to_string(&self) -> String {
        self.name.clone()
    }
}

impl PlasmaMonitor {
    /// Ruft `kscreen-doctor -j` auf, um die Namen aller verbundenen Bildschirme zu ermitteln.
    pub fn list() -> Result<Vec<Self>> {
        let output = Command::new("kscreen-doctor")
            .arg("-j")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()?;

        check_exit_status(output.status)?;

        if output.stdout.is_empty() {
            Ok(vec![])
        } else {
            // Stellt die Struktur der Ausgabe von `kscreen-doctor -j` dar:
            #[derive(Deserialize)]
            struct JsonOutput {
                outputs: Vec<PlasmaMonitor>
            }

            let json_output: JsonOutput = serde_json::from_slice(&output.stdout)?;
            Ok(json_output.outputs)
        }
    }

    /// Ruft `kscreen-doctor -o` auf, um eine menschenlesbare Liste mit Details zu allen verbundenen Displays auszugeben.
    /// Die Ausgabe wird direkt an `stdout` weitergeleitet.
    pub fn show_details() -> Result<()> {
        let status = Command::new("kscreen-doctor")
            .arg("-o")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        check_exit_status(status)
    }

    /// Rotiert diesen Bildschirm zur angegebenen Ausrichtung.
    pub fn rotate(&self, rotation: Rotation) -> Result<()> {
        let status = Command::new("kscreen-doctor")
            .arg(format!("output.{o}.rotation.{rotation}", o = self.name))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        check_exit_status(status)
    }
}


/// Ordnet jeder der vier Bildschirmausrichtungen einen Richtungsvektor zu.
#[derive(Serialize, Deserialize, Clone)]
pub struct OrientationVectors(pub BTreeMap<Rotation, Vec3>);

impl OrientationVectors {
    /// Berechnet die vier Vektoren für die Bildschirmausrichtungen.
    /// # Parameter:
    /// - `down`: Die gemessene Beschleunigung bei einer Rotation von 0° im Uhrzeigersinn
    /// - `left`: Die gemessene Beschleunigung bei einer Rotation von 90° im Uhrzeigersinn
    pub fn from_user_input(down: Vec3, left: Vec3) -> Self {
        // der zu `down` senkrechte Teil von `left`
        let left_orthogonal = left.reject_from(down);

        // Passe die Länge von `left_orthogonal` an, sodass sie der Länge von `down` entspricht.
        let left_final = left_orthogonal.normalize() * down.length();

        Self(BTreeMap::from([
            (Rotation::None, down),
            (Rotation::Inverted, -down),
            (Rotation::Left, left_final),
            (Rotation::Right, -left_final)
        ]))
    }
}


/// Liest Beschleunigungsdaten über die serielle Schnittstelle
/// und rotiert den Bildschirm automatisch, sobald sich die Ausrichtung ändert.
pub fn run_automatic_rotation(orientations: OrientationVectors, monitor: PlasmaMonitor, serial_reader: SerialReader) -> Result<()> {
    let mut current_rotation = Rotation::None;

    // Wiederhole, bis der serielle Datenstrom endet oder ein Fehler auftritt.
    for res in serial_reader {
        let acc = res?;

        // Wähle die Ausrichtung mit der geringsten Differenz zwischen Mess- und Richtungsvektor.
        let (r, _) = orientations.0
            .iter()
            .map(|(&r, &a)| (r, (a - acc).length()))
            .min_by(|(_, a1), (_, a2)| a1.total_cmp(a2))
            .unwrap();

        // `kscreen-doctor` muss nur aufgerufen werden, wenn sich die Rotation geändert hat.
        if r != current_rotation {
            monitor.rotate(r)?;
            current_rotation = r;
        }
    }

    Ok(())
}

/// Hilfsfunktion, die einen Fehler zurückgibt, wenn `kscreen-doctor` einen anderen Exitstatus als 0 meldet.
fn check_exit_status(exit_status: ExitStatus) -> Result<()> {
    if exit_status.success() {
        Ok(())
    } else {
        Err(anyhow!("kscreen-doctor wurde mit Status {exit_status} beendet"))
    }
}
