# SUNLU Filament Tracker

![SUNLU Filament Tracker](docs/github-banner.png)

![Version](https://img.shields.io/badge/Version-1.10.1-00cfd4)
![Platform](https://img.shields.io/badge/Platform-Windows-0078d4)
![Language](https://img.shields.io/badge/Rust-egui-b7410e)
![License](https://img.shields.io/badge/License-Freeware-lightgrey)

## English

**SUNLU Filament Tracker** is a portable Windows application for managing filament spools, remaining stock, print history, AMS assignments, and filament consumption from 3MF files.

> Independent freeware project. It is not affiliated with SUNLU, Bambu Lab, or their official projects. Product and company names are used only to describe compatibility.

### Screenshot

![SUNLU Filament Tracker – English interface](docs/screenshot-en.png)

### Features

- Manage spools by name, manufacturer, material, color, initial weight, remaining weight, empty-spool weight, price, storage location, and notes
- Display remaining filament in grams and percent
- Assign spools to AMS unit and slot
- Read normal `.3mf` files and sliced `.gcode.3mf` files
- Estimate filament consumption from model geometry and print settings
- Use stored slicer values when available
- Print outcomes: successful, failed, partially consumed, no deduction, or manual consumption
- Print history with CSV export
- SQLite database without separate database software
- Database backup and restore
- Automatic retention of up to ten backups
- German and English interface
- Permanent dark design
- Portable Windows application without an installer

### Filament calculation

Sliced `.gcode.3mf` files are evaluated using the stored slicer values whenever available.

For normal 3MF files, the program estimates consumption from model geometry, nozzle diameter, layer height, wall count, infill, support allowance, and material density. This estimate does not replace a full slicer. Purge material, complex support structures, and multicolor changes can only be calculated reliably from a sliced file.

### Build requirements

- Windows 10 or Windows 11
- Rust installed through `rustup`
- Microsoft Visual Studio Build Tools with **Desktop development with C++**

### Build on Windows

1. Download or clone the repository.
2. Run `build_windows.bat`.
3. The executable is created at:

```text
target\release\sunlu_filament_tracker.exe
```

Alternatively:

```powershell
cargo build --release
```

### Data location

The database is stored in the Windows user data folder, for example:

```text
%APPDATA%\Ebert\SunluFilamentTracker\data\filamentbestand.db
```

The exact location is shown under **Settings**.

### Freeware and rights

This software is freeware and may be used free of charge. The unmodified original executable may be redistributed free of charge as long as all copyright and license notices remain intact.

Selling the software, publishing modified versions, redistributing modified source code, renaming it, publishing it under another name, or commercially exploiting the software or any part of it requires prior written permission from **Ralf Ebert**.

The complete terms are available in [LICENSE](LICENSE).

### Liability

Use of the software is at your own risk. No liability is accepted for data loss, incomplete backups, incorrect consumption calculations, or any other damages.

---

## Deutsch

Der **SUNLU Filament Tracker** ist eine portable Windows-Anwendung zur Verwaltung von Filamentrollen, Restbeständen, Druckhistorie, AMS-Zuordnungen und Verbrauchsdaten aus 3MF-Dateien.

> Unabhängiges Freeware-Projekt. Es besteht keine Verbindung zu SUNLU, Bambu Lab oder deren offiziellen Projekten. Produkt- und Firmennamen dienen ausschließlich der Beschreibung der Kompatibilität.

### Screenshot

![SUNLU Filament Tracker – Deutsche Oberfläche](docs/screenshot-de.png)

### Funktionen

- Spulenverwaltung mit Name, Hersteller, Material, Farbe, Anfangsgewicht, Restgewicht, Leergewicht, Preis, Lagerort und Notizen
- Restbestand in Gramm und Prozent
- Zuordnung zu AMS-Einheit und AMS-Fach
- Einlesen normaler `.3mf`-Dateien und geslicter `.gcode.3mf`-Dateien
- Verbrauchsschätzung aus Modellgeometrie und Druckparametern
- Übernahme vorhandener Slicerwerte, sofern diese gespeichert sind
- Druckstatus: erfolgreich, fehlgeschlagen, teilweise verbraucht, nichts abbuchen oder manuelle Eingabe
- Druckhistorie mit CSV-Export
- SQLite-Datenbank ohne zusätzliche Datenbanksoftware
- Datenbanksicherung und Wiederherstellung
- automatische Aufbewahrung von maximal zehn Backups
- deutsche und englische Benutzeroberfläche
- dauerhaftes dunkles Design
- portable Windows-Anwendung ohne Installation

### Verbrauchsberechnung

Geslicte `.gcode.3mf`-Dateien werden bevorzugt anhand der gespeicherten Slicerwerte ausgewertet.

Bei normalen 3MF-Dateien schätzt das Programm den Verbrauch aus Modellgeometrie, Düsengröße, Schichthöhe, Wandanzahl, Infill, Support-Zuschlag und Materialdichte. Diese Schätzung ersetzt keinen vollständigen Slicer. Spülmaterial, komplexe Supportstrukturen und Mehrfarbenwechsel sind nur aus einer geslicten Datei zuverlässig bestimmbar.

### Voraussetzungen zum Erstellen

- Windows 10 oder Windows 11
- Rust über `rustup`
- Microsoft Visual Studio Build Tools mit **Desktopentwicklung mit C++**

### Windows-Build

1. Repository herunterladen oder klonen.
2. `build_windows.bat` starten.
3. Die fertige EXE befindet sich anschließend unter:

```text
target\release\sunlu_filament_tracker.exe
```

Alternativ:

```powershell
cargo build --release
```

### Datenablage

Die Datenbank liegt im Windows-Benutzerdatenordner, zum Beispiel:

```text
%APPDATA%\Ebert\SunluFilamentTracker\data\filamentbestand.db
```

Der genaue Pfad wird unter **Einstellungen** angezeigt.

### Freeware und Rechte

Dieses Programm ist Freeware und darf kostenlos genutzt werden. Die unveränderte Original-EXE darf kostenlos weitergegeben werden, sofern alle Copyright- und Lizenzhinweise erhalten bleiben.

Verkauf, Veröffentlichung geänderter Versionen, Weitergabe geänderter Quelltexte, Umbenennung, Veröffentlichung unter anderem Namen oder kommerzielle Verwertung des Programms oder einzelner Bestandteile erfordern die vorherige schriftliche Genehmigung von **Ralf Ebert**.

Die vollständigen Bedingungen stehen in [LICENSE](LICENSE).

### Haftung

Die Nutzung erfolgt auf eigene Gefahr. Es wird keine Haftung für Datenverlust, unvollständige Sicherungen, fehlerhafte Verbrauchsberechnungen oder sonstige Schäden übernommen.

---

© Ralf Ebert
