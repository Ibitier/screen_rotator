//! Enthält Funktionen zum Bedienen der seriellen Schnittstelle.

use std::{io::{BufRead, BufReader}, time::Duration};

use anyhow::{Result, anyhow};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use serialport::{SerialPort, SerialPortInfo};


const BAUD_RATE: u32 = 9600;


/// Repräsentiert einen noch nicht geöffneten seriellen Anschluss.
/// [`SerialPortInfo`] beinhaltet den Gerätenamen sowie den Typ des Anschlusses.
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialPortName(SerialPortInfo);

/// Benötigt für [`select_serial_port`](crate::args::Args::select_serial_port).
#[allow(clippy::to_string_trait_impl)]
impl ToString for SerialPortName {
    fn to_string(&self) -> String {
        self.0.port_name.clone()
    }
}

impl SerialPortName {
    /// Erzeugt ein neues [`SerialPortName`]-Objekt mit dem angegebenen `port_name`.
    pub fn from_string(port_name: String) -> Self {
        Self(SerialPortInfo { port_name, port_type: serialport::SerialPortType::Unknown })
    }

    /// Listet alle seriellen Anschlüsse auf, die auf diesem System gefunden wurden.
    pub fn list_available() -> Result<Vec<Self>> {
        let v = serialport::available_ports()?
            .into_iter()
            .map(Self)
            .collect();

        Ok(v)
    }

    /// Öffnet diesen seriellen Anschluss.
    pub fn open(&self) -> Result<SerialReader> {
        let port = serialport::new(&self.0.port_name, BAUD_RATE)
            .timeout(Duration::from_secs(20))
            .open()?;

        Ok(SerialReader::new(port))
    }
}


/// Stellt einen geöffneten seriellen Anschluss dar.
/// Dekodiert die eingelesenen Daten zu einem [`Vec3`].
pub struct SerialReader {
    /// Der serielle Datenstrom wird über einen [`BufReader`] gepuffert, um zeilenweises Einlesen zu ermöglichen.
    reader: BufReader<Box<dyn SerialPort>>,

    /// Zwischenspeicher für die aktuell eingelesene Zeile, um ständige Neuallokationen zu vermeiden.
    line: String
}

impl SerialReader {
    /// Erstellt einen neuen [`SerialReader`] aus einem geöffneten [`SerialPort`].
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        Self {
            line: String::new(),
            reader: BufReader::new(port)
        }
    }

    /// Liest eine Zeile vom seriellen Stream.
    /// Bei Erreichen des Endes wird `Ok(None)` zurückgegeben.
    /// Wenn ein Fehler auftritt, wird dieser zurückgegeben.
    fn read_line(&mut self) -> Result<Option<&str>>{
        self.line.clear();
        let bytes_read = self.reader.read_line(&mut self.line)?;

        if bytes_read == 0 {
            Ok(None)
        } else {
            Ok(Some(&self.line))
        }
    }

    /// Gibt den Beschleunigungsvektor zurück oder [`None`], wenn die Zeile kein JSON-Array mit drei Zahlen enthält.
    fn parse_line(line: &str) -> Option<Vec3> {
        serde_json::from_str(line).ok()
    }
}

/// Durch Implementierung des [`Iterator`] traits ist es möglich,
/// die Beschleunigungsvektoren in einem for-loop einzulesen:
/// ```
/// for reading in serial_reader {
///     // ...
/// }
/// ```
impl Iterator for SerialReader {
    type Item = Result<Vec3>;

    fn next(&mut self) -> Option<Result<Vec3>> {
        // lese so lange Zeilen ein, bis eine erfolgreich geparsed werden kann, oder 4 Zeilen fehlerhaft formatiert sind.
        for _ in 0..4 {
            match self.read_line() {
                Err(err) => return Some(Err(err)),
                Ok(None) => return None,
                Ok(Some(str)) => match Self::parse_line(str) {
                    Some(acc) => return Some(Ok(acc)),
                    None => continue
                }
            }
        }

        // Nach zu vielen ungültigen Zeilen wird ein Fehler zurückgegeben, um ein Festfahren zu vermeiden.
        Some(Err(anyhow!("Zu viele ungültige Werte eingelesen")))
    }
}
