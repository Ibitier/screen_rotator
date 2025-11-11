//! Eingangspunkt des Programmes
//!
//! Aus Übersichtsgründen ist die main-Datei kurz gehalten
//! und der Code auf mehrere Module aufgeteilt.


// Verarbeiten von Eingabeargumenten, starten des Programmes
mod args;

// Kommunikation mit dem Arduino
mod serial;

// Ansteuerung des Monitors; Berechnung der Richtungsvektoren
mod monitor;

// Stabilisierung eines Bildes in einem Fenster
mod rotate_image;

use anyhow::Result;
use clap::Parser;


fn main() -> Result<()> {
    // lese Eingabeargumente und führe entsprechenden Befehl aus
    let args = crate::args::Args::parse();
    args.run_selected_mode()
}
