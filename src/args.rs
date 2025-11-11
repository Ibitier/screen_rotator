//! Aufgaben dieses Moduls:
//! - Lesen und Verarbeiten von Eingabeargumenten
//! - Lesen einer (optionalen) Konfigurationsdatei
//! - Interaktive Eingabe von Optionen und Speichern in der Konfigurationsdatei
//! - Ausführen des Programms unter Berücksichtigung der eingegebenen Optionen

use std::{fs, io::ErrorKind, path::{Path, PathBuf}, process, sync::Mutex, thread};

use anyhow::{anyhow, bail, Result};
use glam::Vec3;
use macroquad::color::Color;
use serde::{de::{Unexpected, Visitor}, Deserialize, Deserializer, Serialize, Serializer};

use crate::{monitor::{self, PlasmaMonitor, OrientationVectors, Rotation}, rotate_image, serial::{SerialPortName, SerialReader}};


/// Eine Konvertierung zum/vom JSON-Format ist nur möglich, wenn ein Objekt [`Serialize`]
/// und [`Deserialize`] implementiert.
/// Da dies jedoch nicht für [`Color`] der Fall ist, ist eine manuelle Implementierung notwendig.
/// Rust erlaubt es nicht, traits für externe Structs zu implementieren; daher wird hier ein Wrapper-Struct verwendet.
#[derive(Clone, Copy)]
struct HexColorSerde(Color);

impl HexColorSerde {
    /// Gibt die Farbe im HTML-HEX-Format zurück.
    fn to_hex_str(self) -> String {
        format!(
            "#{r:x}{g:x}{b:x}",
            r = (self.0.r * 255.0) as u8,
            g = (self.0.g * 255.0) as u8,
            b = (self.0.b * 255.0) as u8
        )
    }

    /// Konvertiert einen HTML-HEX-String zu einem [`Color`]-Struct.
    /// Gibt im Falle einer gescheiterten Konvertierung [`None`] zurück.
    fn parse_hex_str(hex_str: &str) -> Option<Color> {
        hex_str
            .to_ascii_lowercase()
            .strip_prefix('#')
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .filter(|&hex_int| hex_int <= 0xFFFFFF)
            .map(Color::from_hex)
    }

    /// Konvertiert einen HTML-HEX-String zu einem [`Color`]-Struct.
    /// Gibt im Falle einer gescheiterten Konvertierung einen Fehler zurück.
    fn try_parse_hex_str(hex_str: &str) -> Result<Color> {
        Self::parse_hex_str(hex_str).ok_or_else(|| anyhow!("Es wurde ein hexadezimaler Farbcode erwartet"))
    }

    /// Ähnlich wie [`parse_hex_str`](Self::parse_hex_str), gibt jedoch einen [`HexColorSerde`]-Wrapper zurück.
    fn from_hex_str(hex_str: &str) -> Option<Self> {
        Self::parse_hex_str(hex_str).map(Self)
    }
}

impl Serialize for HexColorSerde {
    /// Serialisiert zum JSON-Format (als HTML-HEX-String)
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex_str())
    }
}

impl <'d> Deserialize<'d> for HexColorSerde {
    /// Deserialisiert die Farbe aus dem JSON-Format.
    fn deserialize<D: Deserializer<'d>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColorSerVisitor;

        // Um Daten mit `serde-json` zu deserialisieren, muss ein struct angegeben werden, welches [`Visitor`] implementiert.
        impl Visitor<'_> for ColorSerVisitor {
            type Value = HexColorSerde;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "hexadezimaler Farbcode")
            }

            fn visit_str<E: serde::de::Error>(self, hex_str: &str) -> Result<Self::Value, E> {
                HexColorSerde::from_hex_str(hex_str).ok_or_else(|| serde::de::Error::invalid_value(Unexpected::Str(hex_str), &self))
            }
        }

        deserializer.deserialize_str(ColorSerVisitor)
    }
}


/// Inhalt der Konfigurationsdatei.
///
/// Implementiert [`Serialize`] und [`Deserialize`], um eine Konvertierung vom/zum JSON-Format zu ermöglichen.
/// Da eine Konfigurationsdatei nicht alle Felder enthalten muss, sind bei der Verarbeitung
/// der eingelesenen Daten folgende Punkte zu beachten:
/// - [`serde_json`] behandelt ein bei der Deserialisierung nicht gefundenes Feld als `null`.
///   In Rust werden optionale Werte mit [`Option`] gekennzeichnet, wie in diesem Struct bei jedem Feld.
/// - Bei der Serialisierung würden ungesetzte Felder explizit als `null` serialisiert werden.
///   Das würde die Konfigurationsdatei aufblähen und die Lesbarkeit verschlechtern.
///   Durch ```skip_serializing_if = "Option::is_none"``` lässt sich das vermeiden,
///   da Felder ohne Wert übersprungen werden.
///
/// Die Bedeutung der einzelnen Felder ist in [`Args`] dokumentiert.
#[derive(Serialize, Deserialize, Default)]
struct ConfigSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    serial_port: Option<SerialPortName>,

    #[serde(skip_serializing_if = "Option::is_none")]
    monitor: Option<PlasmaMonitor>,

    #[serde(skip_serializing_if = "Option::is_none")]
    image_path: Option<PathBuf>,

    #[serde(skip_serializing_if = "Option::is_none")]
    fullscreen: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    orientations: Option<OrientationVectors>,

    #[serde(skip_serializing_if = "Option::is_none")]
    background_color: Option<HexColorSerde>
}

impl ConfigSettings {
    /// Liest die Datei am angegebenen Dateipfad, deserialisiert den Inhalt,
    /// und gibt bei Erfolg den Inhalt als [`ConfigSettings`] zurück.
    ///
    /// Wenn keine Datei gegeben ist, oder wenn die Datei nicht existiert,
    /// wird stattdessen ein leeres [`ConfigSettings`] zurückgegeben.
    /// Tritt beim Lesen der Datei, oder bei der Deserialisierung ein Fehler auf,
    /// wird dieser zurückgegeben.
    fn from_file_or_default(file: &Option<PathBuf>) -> Result<Self> {
        let Some(file) = file else { return Ok(Default::default()) };

        match fs::read_to_string(file) {
            Ok(file_contents) => match serde_json::from_str(&file_contents) {
                Ok(s) => Ok(s),
                Err(e) => Err(anyhow!(e))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
            Err(e) => Err(anyhow!(e))
        }
    }

    /// Serialisiert diese [`ConfigSettings`] in die angegebene Datei.
    fn save_to_file(&self, file: &Path) -> Result<()> {
        let json_string = serde_json::to_string_pretty(self)?;
        fs::write(file, json_string)?;

        Ok(())
    }
}


/// Hilfsstruktur, die die automatische Verarbeitung von Eingabeargumenten über
/// das [`clap`]-Interface ermöglicht.
#[derive(clap::Parser)]
#[command(name = "screen_rotator", about = None)]
pub struct Args {
    /// Dateipfad einer Konfigurationsdatei im JSON-Format
    #[arg(long)]
    config: Option<PathBuf>,

    /// Name der seriellen Schnittstelle, an der der Arduino angeschlossen ist
    #[arg(long)]
    serial_port: Option<String>,

    /// Verhindert die interaktive Eingabe von Optionen (geeignet für automatische Skripte)
    #[arg(long)]
    non_interactive: bool,

    /// Erzwingt die (interaktive) Neuberechnung der Richtungsvektoren
    #[arg(long)]
    recalculate_vectors: bool,

    /// Der auszuführende Befehl
    #[command(subcommand)]
    mode: Commands
}

/// Diese Struktur beschreibt alle Befehle, die das Programm ausführen kann.
#[derive(clap::Subcommand, Clone)]
enum Commands {
    /// Liest die Rotationsdaten und rotiert den Bildschirm automatisch um Vielfache von 90°
    RotateMonitor {
        /// Name des Bildschirms
        #[arg(long)]
        monitor: Option<String>,
    },

    /// Liest die Rotationsdaten und stabilisiert ein Bild, sodass es immer parallel zum Erdboden ausgerichtet bleibt
    RotateImage {
        /// Pfad zur Bilddatei
        #[arg(long)]
        image_path: Option<PathBuf>,

        /// Öffnet das Fenster im Vollbildmodus
        #[arg(long, default_value_t = false)]
        fullscreen: bool,

        /// Hexcode für die Hintergrundfarbe des Fensters
        #[arg(long, value_parser = HexColorSerde::try_parse_hex_str, default_value = "#000000")]
        background_color: Option<Color>
    }
}

impl Args {
    /// Haupteintrittspunkt des Programms, nachdem alle Eingabeargumente verarbeitet wurden.
    /// Liest eine Konfigurationsdatei ein, wenn diese angegeben wurde, und führt den als Eingabeargument übergebenen Befehl aus.
    pub fn run_selected_mode(self) -> Result<()> {
        // Lese die Konfigurationsdatei ein.
        // Wenn keine angegeben wurde, sind alle Felder [`None`].
        let mut config = ConfigSettings::from_file_or_default(&self.config)?;

        // Behalte im Überblick, ob interaktive Änderungen an der Konfiguration durchgeführt worden sind.
        // Wenn das der Fall ist, wird am Ende angeboten, die aktuelle Konfiguration in einer Datei zu speichern.
        let mut user_input_made = false;

        // Überprüft, ob ein Monitorname oder Bilddateipfad in den Eingabeargumenten enthalten ist
        // und ob dieser für den jeweiligen Modus benötigt wird.
        let (args_monitor, args_image_path, monitor_required, image_path_required) = match &self.mode {
            Commands::RotateMonitor { monitor } => (monitor.as_deref(), None, true, false),
            Commands::RotateImage { image_path, .. } => (None, image_path.as_deref(), false, true),
        };

        // Die nächsten vier Abschnitte folgen alle demselben Schema:
        // - Wenn ein Wert in den Eingabeargumenten angegeben wurde, hat er Vorrang.
        // - Andernfalls wird der gespeicherte Wert aus der Konfiguration verwendet.
        // - Falls auch dort nichts hinterlegt ist und das Programm interaktiv ausgeführt wird, wird der Wert interaktiv eingelesen.
        // - Im nicht-interaktiven Modus wird ein Fehler zurückgegeben, sofern der Wert für den ausgewählten Modus benötigt wird.
        // Bei interaktiven Eingaben wird der neue Wert in der Konfiguration zwischengespeichert.
        // Wenn der Benutzer das Speichern der Konfiguration ablehnt, bleibt die Datei unverändert.
        let mut serial_reader = {
            let serial_port = if let Some(name) = self.serial_port {
                SerialPortName::from_string(name)
            } else if let Some(port) = config.serial_port {
                port
            } else if !self.non_interactive {
                user_input_made = true;
                Self::select_serial_port()?
            } else {
                bail!("Serieller Anschluss wurde nicht angegeben")
            };
            let reader = serial_port.open()?;
            config.serial_port = Some(serial_port);
            reader
        };

        let monitor = {
            // Die Rotation des gesamten Monitors ist nur unter KDE Plasma unterstützt.
            // Wenn kein Plasma erkannt wurde, wird ein Fehler zurückgegeben.
            let monitor = match Self::check_rotation_supported() {
                Ok(()) => if let Some(name) = args_monitor {
                        Ok(PlasmaMonitor { name: name.to_string() })
                    } else if let Some(monitor) = config.monitor {
                        Ok(monitor)
                    } else if monitor_required && !self.non_interactive {
                        user_input_made = true;
                        Ok(Self::select_monitor()?)
                    } else {
                        Err(anyhow!("Monitor wurde nicht angegeben"))
                    },
                Err(e) => Err(e)
            };
            config.monitor = monitor.as_ref().ok().cloned();
            monitor
        };

        let orientations = {
            // Die Vektoren können nicht per Eingabeargument übergeben werden.
            // Daher wird dieser Schritt hier übersprungen.
            let orientations = if let Some(orientations) = config.orientations && !self.recalculate_vectors {
                orientations
            } else if !self.non_interactive || self.recalculate_vectors {
                user_input_made = true;
                Self::calculate_vectors(&mut serial_reader, monitor.as_ref().ok())?
            } else {
                bail!("Richtungsvektoren wurden nicht angegeben")
            };
            config.orientations = Some(orientations.clone());
            orientations
        };

        let image_path = {
            let image_path: Result<PathBuf> = if let Some(image_path) = args_image_path {
                Ok(image_path.to_path_buf())
            } else if let Some(image_path) = config.image_path {
                Ok(image_path)
            } else if image_path_required && !self.non_interactive {
                user_input_made = true;
                Ok(Self::select_image()?)
            } else {
                Err(anyhow!("Bildpfad wurde nicht angegeben"))
            };
            config.image_path = image_path.as_ref().ok().cloned();
            image_path
        };

        // Wenn die Konfiguration interaktiv geändert wurde:
        // optionales Speichern anbieten.
        if user_input_made {
            Self::save_config(config, &self.config)?;
        }

        // führe den ausgewählten Modus aus
        match self.mode {
            Commands::RotateMonitor { .. } => {
                monitor::run_automatic_rotation(
                    orientations,
                    monitor?,
                    serial_reader
                )
            }

            Commands::RotateImage { image_path: _, fullscreen, background_color } => {
                rotate_image::run_image_stabilizer(
                    fullscreen,
                    background_color.unwrap(),
                    &image_path?,
                    orientations,
                    serial_reader,
                )
            }
        }
    }

    /// Zeigt alle verfügbaren Bildschirme an und erlaubt die interaktive Auswahl eines davon.
    /// Auf Wunsch werden detaillierte Informationen zu den Displays angezeigt.
    fn select_monitor() -> Result<PlasmaMonitor> {
        loop {
            let mut monitors = monitor::PlasmaMonitor::list()?;

            let i = dialoguer::Select::new()
                .with_prompt("Monitor auswählen")
                .item("Detaillierte Infos anzeigen")
                .items(&monitors)
                .interact()?;

            if i == 0 {
                monitor::PlasmaMonitor::show_details()?;
            } else {
                return Ok(monitors.swap_remove(i-1));
            }
        }
    }

    /// Zeigt die Namen aller seriellen Schnittstellen an und ermöglicht die interaktive Auswahl.
    fn select_serial_port() -> Result<SerialPortName> {
        let mut serial_ports = SerialPortName::list_available()?;

        let i = dialoguer::Select::new()
            .with_prompt("Seriellen Anschluss auswählen")
            .items(&serial_ports)
            .interact()?;

        Ok(serial_ports.swap_remove(i))
    }

    /// Erlaubt die interaktive Eingabe eines Bilddateipfads.
    /// Ob die angegebene Datei tatsächlich ein Bild ist, wird erst später im Programm geprüft.
    fn select_image() -> Result<PathBuf> {
        let path = dialoguer::Input::new()
            .with_prompt("Bildspeicherort eingeben")
            .validate_with(|path: &String| {
                match fs::exists(path) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err("Angegebener Speicherort existiert nicht".to_string()),
                    Err(e) => Err(format!("Auf Datei kann nicht zugegriffen werden: {e}"))
                }
            })
            .interact_text()?;

        Ok(PathBuf::from(path))
    }

    /// Misst interaktiv die Beschleunigungen bei den Rotationen `down` und `left`
    /// und ruft [`OrientationVectors::from_user_input`] auf, um die Richtungsvektoren zu berechnen.
    fn calculate_vectors(serial_reader: &mut SerialReader, monitor: Option<&PlasmaMonitor>) -> Result<OrientationVectors> {
        // Ein Mutex wird benötigt, um Daten zwischen Threads zu teilen.
        let acceleration_mutex = Mutex::new((Vec3::default(), false));

        thread::scope(|s| {
            // Hintergrundthread, der fortlaufend die Beschleunigung einliest.
            let handle = s.spawn(|| loop {
                let acceleration = serial_reader.next().ok_or_else(|| anyhow!("Ende des Datenstroms erreicht"))??;
                let mut guard = acceleration_mutex.lock().unwrap();

                if guard.1 { return anyhow::Ok(()); }
                guard.0 = acceleration;
            });

            let acc_down = {
                let cont = dialoguer::Confirm::new()
                    .with_prompt("Bildschirm nach unten drehen. Fortfahren?")
                    .default(true)
                    .wait_for_newline(true)
                    .interact()?;

                if !cont { process::exit(1); }
                // Lese die aktuelle Beschleunigung aus dem Mutex.
                let guard = acceleration_mutex.lock().unwrap();
                guard.0
            };

            let acc_left = {
                // Wenn ein Monitor angegeben ist, wird dieser nach links gedreht,
                // um die korrekte Drehrichtung zu verdeutlichen.
                if let Some(monitor) = monitor {
                    monitor.rotate(Rotation::Left)?;
                }

                let cont = dialoguer::Confirm::new()
                    .with_prompt("Bildschirm nach links drehen. Fortfahren?")
                    .default(true)
                    .wait_for_newline(true)
                    .interact()?;

                if !cont { process::exit(1); }
                // Lese die aktuelle Beschleunigung aus dem Mutex und signalisiere dem Hintergrundthread, dass er anhalten soll.
                let mut guard = acceleration_mutex.lock().unwrap();
                guard.1 = true;
                guard.0
            };

            // Drehe den Monitor wieder in die Ausgangslage.
            if let Some(monitor) = monitor {
                monitor.rotate(Rotation::None)?;
            }

            // Berechne die Richtungsvektoren.
            let vectors = OrientationVectors::from_user_input(acc_down, acc_left);

            match handle.join() {
                Ok(Ok(())) => Ok(vectors),
                Ok(Err(e)) => Err(e),
                Err(panic) => std::panic::panic_any(panic),
            }
        })
    }

    /// Bietet an, geänderte Konfigurationswerte in der angegebenen Datei zu speichern.
    /// Wenn kein Dateipfad vorliegt, wird dieser interaktiv abgefragt.
    fn save_config(config: ConfigSettings, path: &Option<PathBuf>) -> Result<()> {
        let mut prompt = "Konfiguration mit neuen Werten überschreiben";

        loop {
            let save = dialoguer::Confirm::new()
                .with_prompt(prompt)
                .interact()?;

            if !save { return Ok(()); }

            let mut input = dialoguer::Input::new()
                .with_prompt("Dateispeicherort eingeben");

            prompt = "Erneut versuchen";

            if let Some(path) = path {
                input = input.with_initial_text(path.to_string_lossy());
            }

            let path: String = input.interact_text()?;
            if path.is_empty() {
                eprintln!("kein Speicherort angegeben");
                continue;
            }

            match config.save_to_file(Path::new(&path)) {
                Ok(()) => return Ok(()),
                Err(e) => eprintln!("Konfiguration konnte nicht gespeichert werden: {e}")
            }
        }
    }

    /// Gibt einen Fehler zurück, wenn die Rotation des Displays nicht unterstützt ist.
    /// Unterstützt wird ausschließlich die Desktop-Umgebung `Plasma` von KDE unter Linux.
    fn check_rotation_supported() -> Result<()> {
        // CFGs (Compiler Flags) können verwendet werden, um während der Kompilierung das verwendete Betriebssystem zu untersuchen

        #[cfg(not(target_os = "linux"))]
        {
            Err(anyhow!("Bildschirmrotation wird nur unter Linux unterstützt"))
        }

        #[cfg(target_os = "linux")]
        {
            // Die Umgebungsvariable "DESKTOP_SESSION" enthält die aktuell verwendete Desktop-Umgebung.
            // Wenn Plasma aktiv ist, ist der Wert "plasma".
            match std::env::var("DESKTOP_SESSION")?.as_str() {
                "plasma" => Ok(()),
                env => Err(anyhow!("Desktop-Umgebung \"{env}\" nicht unterstützt")),
            }
        }
    }
}
