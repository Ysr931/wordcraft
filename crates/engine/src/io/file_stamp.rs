//! Eigene Ergänzung im Zweig `eigen`: Wurde die Datei eines offenen Dokuments inzwischen von
//! außen geändert, zum Beispiel von Syncthing mit der neueren Fassung eines anderen Rechners?
//! Ein normales Speichern würde diese Fassung still ersetzen, und das Sync-Programm verteilt das
//! Ergebnis dann auf alle Rechner. `file.save` verweigert das deshalb, solange niemand mit
//! `"overwrite": true` ausdrücklich zustimmt. Die Oberfläche fragt vorher nach.

/// Was sich eine Sitzung beim Lesen und Schreiben über die Datei merkt. Größe und Änderungszeit
/// lassen sich vor jedem Speichern günstig lesen, und Syncthing übernimmt die Änderungszeit des
/// anderen Rechners mit dem Inhalt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: std::time::SystemTime,
}

/// Die Fehlermeldung, mit der `file.save` ablehnt. [`changed_elsewhere`] erkennt sie wieder.
pub const CHANGED_ELSEWHERE: &str = "The file changed on disk after it was opened or last saved here, so it was not replaced. Save it under another name, or pass \"overwrite\": true.";

/// Der aktuelle Stand der Datei; `None`, wenn sie fehlt oder nicht lesbar ist.
#[cfg(not(target_arch = "wasm32"))]
pub fn file_stamp(path: &std::path::Path) -> Option<FileStamp> {
    crate::file_access::check_read(&path.to_string_lossy()).ok()?;
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp { len: meta.len(), modified: meta.modified().ok()? })
}

/// Im Browser gibt es keine Dateipfade, also auch nichts zu vergleichen.
#[cfg(target_arch = "wasm32")]
pub fn file_stamp(_path: &std::path::Path) -> Option<FileStamp> {
    None
}

/// Bedeutet dieser Fehler, dass `file.save` die von außen geänderte Datei nicht ersetzt hat?
pub fn changed_elsewhere(err: &str) -> bool {
    err.contains(CHANGED_ELSEWHERE)
}

impl crate::Session {
    /// Hat sich die Datei des aktiven Dokuments geändert, seit diese Sitzung sie zuletzt gelesen
    /// oder geschrieben hat? Eine gelöschte Datei zählt nicht: Speichern legt sie neu an, dabei
    /// geht nichts verloren.
    pub fn file_changed_elsewhere(&self) -> bool {
        let (Some(path), Some(known)) = (&self.path, self.file_stamp) else { return false };
        file_stamp(path).is_some_and(|now| now != known)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{changed_elsewhere, file_stamp};
    use crate::Session;

    fn ordner(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wordcraft-file-stamp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sitzung(text: &str) -> Session {
        Session::new(wordcraft_doc::Document::from_text(text))
    }

    /// Eine andere Sitzung ersetzt die Datei so, wie Syncthing es tut: neue Datei, dann umbenennen.
    fn von_aussen_ersetzen(path: &std::path::Path, text: &str) {
        let tmp = path.with_extension("fremd.docx");
        sitzung(text).run("file.save", &json!({"path": tmp.to_string_lossy()})).unwrap();
        std::fs::rename(&tmp, path).unwrap();
    }

    fn inhalt(path: &std::path::Path) -> String {
        crate::io::open_path(path).unwrap().plain_text(wordcraft_doc::StoryRef::Body)
    }

    fn geoeffnet(path: &std::path::Path) -> Session {
        let mut s = sitzung("");
        s.run("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        s
    }

    #[test]
    fn speichern_ersetzt_keine_von_aussen_geaenderte_datei() {
        let dir = ordner("abgelehnt");
        let path = dir.join("a.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Neu von Mac A");
        s.run("text.insert", &json!({"text": "Mac B "})).unwrap();

        assert!(s.file_changed_elsewhere());
        let e = s.run("file.save", &json!({})).unwrap_err().to_string();
        assert!(changed_elsewhere(&e), "{e}");
        assert!(inhalt(&path).contains("Neu von Mac A"), "die Datei bleibt, wie sie ist");
        assert!(s.dirty, "das Dokument gilt weiter als ungespeichert");

        // Derselbe Pfad, ausdrücklich genannt, ist kein Schlupfloch.
        let e = s.run("file.save", &json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
        assert!(changed_elsewhere(&e), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nach_der_zustimmung_wird_ueberschrieben_und_nicht_mehr_gefragt() {
        let dir = ordner("zustimmung");
        let path = dir.join("a.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Neu von Mac A");
        s.run("text.insert", &json!({"text": "Mac B "})).unwrap();

        s.run("file.save", &json!({"overwrite": true})).unwrap();
        assert!(inhalt(&path).contains("Mac B"));
        assert!(!s.dirty);
        assert!(!s.file_changed_elsewhere());
        s.run("text.insert", &json!({"text": "Mehr "})).unwrap();
        s.run("file.save", &json!({})).unwrap();
        assert!(inhalt(&path).contains("Mehr"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nur die Änderungszeit weicht ab, die Größe ist gleich.
    #[test]
    fn gleiche_groesse_mit_anderer_zeit_zaehlt_als_aenderung() {
        let dir = ordner("zeit");
        let path = dir.join("a.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let s = geoeffnet(&path);
        let vorher = file_stamp(&path).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(file.metadata().unwrap().modified().unwrap() + std::time::Duration::from_secs(5)).unwrap();
        drop(file);
        assert_ne!(file_stamp(&path).unwrap(), vorher);
        assert!(s.file_changed_elsewhere());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normales_arbeiten_loest_nichts_aus() {
        let dir = ordner("normal");
        let path = dir.join("a.docx");
        let mut s = sitzung("Text");
        // Noch nie gespeichert: nichts zu vergleichen.
        assert!(!s.file_changed_elsewhere());
        s.run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        for i in 0..3 {
            s.run("text.insert", &json!({"text": format!("{i} ")})).unwrap();
            s.run("file.save", &json!({})).unwrap();
        }
        assert!(!s.file_changed_elsewhere());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ein Mac hat die Datei gelöscht: Speichern legt sie wieder an, verloren geht dabei nichts.
    #[test]
    fn eine_geloeschte_datei_wird_neu_angelegt() {
        let dir = ordner("geloescht");
        let path = dir.join("a.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&path);
        std::fs::remove_file(&path).unwrap();
        s.run("text.insert", &json!({"text": "Noch da "})).unwrap();
        s.run("file.save", &json!({})).unwrap();
        assert!(inhalt(&path).contains("Noch da"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Speichern unter einem neuen Namen fasst die geänderte Datei nicht an, und danach gilt der
    /// Stand der neuen Datei.
    #[test]
    fn speichern_unter_neuem_namen_laesst_beide_fassungen_bestehen() {
        let dir = ordner("kopie");
        let path = dir.join("a.docx");
        let kopie = dir.join("b.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Neu von Mac A");
        s.run("text.insert", &json!({"text": "Mac B "})).unwrap();

        s.run("file.save", &json!({"path": kopie.to_string_lossy()})).unwrap();
        assert!(inhalt(&path).contains("Neu von Mac A"));
        assert!(inhalt(&kopie).contains("Mac B"));
        assert_eq!(s.path.as_deref(), Some(kopie.as_path()));
        assert!(!s.file_changed_elsewhere());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Wer in „Speichern unter“ die Datei selbst auswählt, hat dem Ersetzen schon zugestimmt.
    #[test]
    fn speichern_unter_mit_derselben_datei_ersetzt_sie() {
        let dir = ordner("saveas");
        let path = dir.join("a.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Neu von Mac A");
        s.run("text.insert", &json!({"text": "Mac B "})).unwrap();
        s.run("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(inhalt(&path).contains("Mac B"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Umbenennen behält den Stand: Es ist dieselbe Datei.
    #[test]
    fn umbenennen_ist_keine_aenderung() {
        let dir = ordner("rename");
        let path = dir.join("a.docx");
        sitzung("Alt").run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&path);
        s.run("file.rename", &json!({"name": "c"})).unwrap();
        assert!(!s.file_changed_elsewhere());
        s.run("text.insert", &json!({"text": "Mehr "})).unwrap();
        s.run("file.save", &json!({})).unwrap();
        assert!(inhalt(&dir.join("c.docx")).contains("Mehr"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Jedes offene Dokument merkt sich seinen eigenen Stand.
    #[test]
    fn jedes_dokument_hat_seinen_eigenen_stand() {
        let dir = ordner("zwei");
        let (a, b) = (dir.join("a.docx"), dir.join("b.docx"));
        sitzung("Eins").run("file.save", &json!({"path": a.to_string_lossy()})).unwrap();
        sitzung("Zwei").run("file.save", &json!({"path": b.to_string_lossy()})).unwrap();
        let mut s = geoeffnet(&a);
        let erstes = s.document_id();
        s.run("text.insert", &json!({"text": "x"})).unwrap();
        s.run("file.open", &json!({"path": b.to_string_lossy()})).unwrap();
        assert_ne!(s.document_id(), erstes, "das zweite Dokument öffnet neben dem geänderten");
        von_aussen_ersetzen(&a, "Neu von Mac A");
        assert!(!s.file_changed_elsewhere(), "b.docx wurde nicht geändert");
        s.activate(erstes).unwrap();
        assert!(s.file_changed_elsewhere(), "a.docx wurde geändert");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
