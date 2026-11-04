//! Eigene Ergänzung im Zweig `eigen`: Die Nachfrage, bevor Speichern eine von außen geänderte
//! Datei ersetzt (siehe `wordcraft_engine::io::file_stamp`). Die Texte stehen hier auf Deutsch und
//! Englisch, weil die Übersetzungen sonst im fremden Repo `craft-i18n` lägen.

use egui::Ui;

use super::{Value, WordApp, file_dialogs, json};
use crate::dialogs::Dialog;

/// Der deutsche Text bei deutscher Oberfläche, sonst der englische.
fn say(de: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::current().code() == "de" { de } else { tl!(en) }
}

impl WordApp {
    /// Muss dieser Befehl erst nachfragen? Nur `file.save` auf die eigene Datei des Dokuments, und
    /// nur ohne ausdrückliche Zustimmung.
    pub(super) fn save_needs_asking(&self, id: &str, params: &Value) -> bool {
        id == "file.save"
            && self.has_document()
            && params.get("overwrite").and_then(Value::as_bool) != Some(true)
            && params.get("path").and_then(Value::as_str).is_none_or(|p| self.session.path.as_deref() == Some(std::path::Path::new(p)))
            && self.session.file_changed_elsewhere()
    }

    /// Zeigt die Nachfrage für das aktive Dokument und weckt dafür das Fenster: Sie darf nicht auf
    /// die nächste Eingabe warten, und AutoSpeichern läuft auch, während das Fenster verdeckt ist
    /// und nichts gezeichnet wird.
    pub(super) fn ask_file_changed(&mut self) {
        self.dialog = Some(Dialog::FileChanged { name: self.prompt_name(), document: self.session.document_id() });
        if let Some(ctx) = self.ctx.clone() {
            let viewport = self.active_viewport();
            if ctx.input_for(viewport, |i| i.viewport().visible()) != Some(true) {
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::RequestPaintWhileHidden);
            }
            ctx.request_repaint_of(viewport);
            ctx.request_repaint();
        }
    }

    /// AutoSave hat die geänderte Datei nicht ersetzt und bleibt für das Dokument aus, bis es
    /// gespeichert wurde. Ein anderer offener Dialog bleibt stehen; dann sagt es die Statuszeile,
    /// und das nächste Speichern fragt nach.
    pub(super) fn autosave_met_changed_file(&mut self) {
        self.status(say(
            "Die Datei wurde von außen geändert. AutoSpeichern ist für dieses Dokument aus, bis du es gespeichert hast.",
            "The file changed on disk. AutoSave is off for this document until you save it.",
        ));
        if self.dialog.is_none() {
            self.ask_file_changed();
        }
    }

    /// `ui.fileChanged`: `saveAs` fragt nach einem neuen Namen, `overwrite` ersetzt die Datei,
    /// alles andere lässt die Datei und das Dokument, wie sie sind.
    pub(super) fn answer_file_changed(&mut self, answer: &str) -> Result<Value, String> {
        let Some(Dialog::FileChanged { document, .. }) = self.dialog.take() else {
            return Err("no file-changed prompt is open".into());
        };
        if document != self.session.document_id() {
            return Err("the document the prompt asked about has been replaced".into());
        }
        match answer {
            "saveAs" => match self.save_as(file_dialogs::AfterSave::Nothing) {
                file_dialogs::Asked::Done(r) => r,
                file_dialogs::Asked::Pending => Ok(json!({"pending": "saveAs"})),
                file_dialogs::Asked::Busy => Ok(json!({"saved": false})),
            },
            "overwrite" => {
                let r = self.run("file.save", json!({"overwrite": true}));
                if r.is_ok() {
                    self.ui.backstage = false;
                    self.canvas.want_focus = true;
                }
                r
            }
            _ => Ok(json!({"saved": false})),
        }
    }
}

/// Der Dialog. Der hervorgehobene Knopf „Unter neuem Namen speichern“ hat den Fokus und verliert
/// nichts: Rutscht beim Tippen die Eingabetaste durch, öffnet sich nur der Speicherdialog.
pub(crate) fn file_changed_dialog(app: &mut WordApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let Dialog::FileChanged { name, .. } = d else {
        return false;
    };
    let heading = crate::i18n::fmt(say("„{name}“ wurde inzwischen geändert", "{name} has changed in the meantime"), &[("name", name)]);
    ui.label(egui::RichText::new(heading).font(crate::theme::semibold(15.0)));
    ui.label(say(
        "Die Datei wurde seit dem Öffnen oder letzten Speichern von außen verändert, zum Beispiel auf einem anderen Rechner. Beim Überschreiben ersetzt deine Fassung diese Änderung.",
        "The file was changed from outside since you opened or last saved it, for example on another computer. If you overwrite it, your version replaces that change.",
    ));
    ui.label(say(
        "Unter neuem Namen speichern behält beide Fassungen, dann kannst du sie vergleichen.",
        "Saving under a new name keeps both versions, so you can compare them.",
    ));
    ui.add_space(8.0);
    let [cancel, overwrite, save_as] = ui
        .horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cancel = ui.button(say("Abbrechen", "Cancel"));
                let overwrite = ui.button(say("Überschreiben", "Overwrite"));
                let save_as = ui.add(crate::widgets::primary_button(ui, say("Unter neuem Namen speichern…", "Save As…")));
                [cancel, overwrite, save_as]
            })
            .inner
        })
        .inner;
    if !(cancel.has_focus() || overwrite.has_focus() || save_as.has_focus()) {
        save_as.request_focus();
    }
    let answer = if ui.input(|i| i.key_pressed(egui::Key::Escape)) || cancel.clicked() {
        "cancel"
    } else if overwrite.clicked() {
        "overwrite"
    } else if save_as.clicked() {
        "saveAs"
    } else {
        return false;
    };
    // `ui.fileChanged` liest die Nachfrage aus `app.dialog`, das `show` herausgenommen hat.
    app.dialog = Some(d.clone());
    let _ = app.run("ui.fileChanged", json!({"answer": answer}));
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_engine::Session;

    use crate::WordApp;
    use crate::services::Services;

    fn ordner(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wordcraft-ui-changed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn inhalt(path: &std::path::Path) -> String {
        wordcraft_engine::io::open_path(path).unwrap().plain_text(wordcraft_doc::StoryRef::Body)
    }

    fn speichern_als(path: &std::path::Path, text: &str) {
        Session::new(wordcraft_doc::Document::from_text(text)).run("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
    }

    /// Wie Syncthing: neue Datei daneben schreiben, dann über die alte umbenennen.
    fn von_aussen_ersetzen(path: &std::path::Path, text: &str) {
        let tmp = path.with_extension("fremd.docx");
        speichern_als(&tmp, text);
        std::fs::rename(&tmp, path).unwrap();
    }

    /// Ein geöffnetes Dokument mit Änderung, dessen Datei inzwischen von außen ersetzt wurde.
    fn veraltet(dir: &std::path::Path) -> (WordApp, std::path::PathBuf) {
        let path = dir.join("a.docx");
        speichern_als(&path, "Alt");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("file.open", json!({"path": path.to_string_lossy()})).unwrap();
        von_aussen_ersetzen(&path, "Neu von Mac A");
        a.run("text.insert", json!({"text": "Mac B "})).unwrap();
        (a, path)
    }

    fn dialog(a: &WordApp) -> Option<&'static str> {
        a.dialog.as_ref().map(|d| d.name())
    }

    #[test]
    fn speichern_fragt_nach_und_laesst_die_datei_in_ruhe() {
        let dir = ordner("fragt");
        let (mut a, path) = veraltet(&dir);
        let r = a.run("file.save", json!({})).unwrap();
        assert_eq!(r["pending"], "fileChanged");
        assert_eq!(dialog(&a), Some("fileChanged"));
        assert!(inhalt(&path).contains("Neu von Mac A"));
        assert!(a.session.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ueberschreiben_ersetzt_die_datei_und_autosave_laeuft_weiter() {
        let dir = ordner("ueberschreiben");
        let (mut a, path) = veraltet(&dir);
        a.run("file.save", json!({})).unwrap();
        a.run("ui.fileChanged", json!({"answer": "overwrite"})).unwrap();
        assert_eq!(dialog(&a), None);
        assert!(inhalt(&path).contains("Mac B"));
        assert!(!a.session.dirty);
        assert!(a.saved_here(), "AutoSave deckt die Datei wieder ab");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn abbrechen_laesst_alles_wie_es_ist() {
        let dir = ordner("abbrechen");
        let (mut a, path) = veraltet(&dir);
        a.run("file.save", json!({})).unwrap();
        a.run("ui.fileChanged", json!({"answer": "cancel"})).unwrap();
        assert_eq!(dialog(&a), None);
        assert!(inhalt(&path).contains("Neu von Mac A"));
        assert!(a.session.dirty);
        // Das nächste Speichern fragt wieder.
        a.run("file.save", json!({})).unwrap();
        assert_eq!(dialog(&a), Some("fileChanged"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn neuer_name_behaelt_beide_fassungen() {
        let dir = ordner("neuer-name");
        let (mut a, path) = veraltet(&dir);
        let kopie = dir.join("b.docx");
        let gewaehlt = kopie.to_string_lossy().to_string();
        a.services.pick_save = Some(Box::new(move |_| Some(gewaehlt.clone())));
        a.run("file.save", json!({})).unwrap();
        a.run("ui.fileChanged", json!({"answer": "saveAs"})).unwrap();
        assert!(inhalt(&path).contains("Neu von Mac A"), "die Fassung von Mac A bleibt");
        assert!(inhalt(&kopie).contains("Mac B"), "die eigene Fassung liegt daneben");
        assert_eq!(a.session.path.as_deref(), Some(kopie.as_path()));
        assert!(!a.session.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// In „Speichern unter“ die Originaldatei zu wählen ist kein Umweg zurück in die Nachfrage:
    /// Der Dateidialog hat schon gefragt.
    #[test]
    fn speichern_unter_auf_dieselbe_datei_fragt_nicht_doppelt() {
        let dir = ordner("dieselbe");
        let (mut a, path) = veraltet(&dir);
        let gewaehlt = path.to_string_lossy().to_string();
        a.services.pick_save = Some(Box::new(move |_| Some(gewaehlt.clone())));
        a.save_as_dialog();
        assert_eq!(dialog(&a), None);
        assert!(inhalt(&path).contains("Mac B"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosave_ersetzt_nichts_und_fragt() {
        let dir = ordner("autosave");
        let path = dir.join("a.docx");
        speichern_als(&path, "Alt");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("file.open", json!({"path": path.to_string_lossy()})).unwrap();
        // AutoSave deckt nur Dateien ab, die hier schon gespeichert wurden.
        a.run("file.save", json!({})).unwrap();
        assert!(a.saved_here());
        von_aussen_ersetzen(&path, "Neu von Mac A");
        a.run("text.insert", json!({"text": "Mac B "})).unwrap();

        a.autosave_tick(crate::clock::now_ms() + 10_000.0);
        assert!(inhalt(&path).contains("Neu von Mac A"), "AutoSave ersetzt die Datei nicht");
        assert_eq!(dialog(&a), Some("fileChanged"));
        assert!(!a.saved_here(), "AutoSave bleibt für das Dokument aus");
        assert!(a.status_msg.as_ref().is_some_and(|(m, _)| m.contains("AutoSave")));

        // Abgebrochen: Weitere Durchläufe schreiben nichts und öffnen die Nachfrage nicht erneut.
        a.run("ui.fileChanged", json!({"answer": "cancel"})).unwrap();
        a.autosave_tick(crate::clock::now_ms() + 20_000.0);
        assert_eq!(dialog(&a), None);
        assert!(inhalt(&path).contains("Neu von Mac A"));

        // Nach dem Überschreiben läuft AutoSave wieder.
        a.run("file.save", json!({})).unwrap();
        a.run("ui.fileChanged", json!({"answer": "overwrite"})).unwrap();
        a.run("text.insert", json!({"text": "Mehr "})).unwrap();
        a.autosave_tick(crate::clock::now_ms() + 30_000.0);
        assert!(inhalt(&path).contains("Mehr"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ein anderer offener Dialog wird nicht ersetzt.
    #[test]
    fn autosave_verdraengt_keinen_offenen_dialog() {
        let dir = ordner("anderer-dialog");
        let path = dir.join("a.docx");
        speichern_als(&path, "Alt");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("file.open", json!({"path": path.to_string_lossy()})).unwrap();
        a.run("file.save", json!({})).unwrap();
        von_aussen_ersetzen(&path, "Neu von Mac A");
        a.run("text.insert", json!({"text": "Mac B "})).unwrap();
        a.dialog = crate::dialogs::Dialog::open("about", &mut a);
        let vorher = dialog(&a);
        assert!(vorher.is_some());
        a.autosave_tick(crate::clock::now_ms() + 10_000.0);
        assert_eq!(dialog(&a), vorher);
        assert!(a.status_msg.as_ref().is_some_and(|(m, _)| m.contains("AutoSave")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Beim Schließen: Speichern führt zur Nachfrage, das Dokument bleibt offen, nichts wird ersetzt.
    #[test]
    fn schliessen_mit_speichern_fragt_und_bleibt_offen() {
        let dir = ordner("schliessen");
        let (mut a, path) = veraltet(&dir);
        assert!(!a.close_requested());
        assert_eq!(dialog(&a), Some("saveChanges"));
        a.run("ui.saveChanges", json!({"answer": "save"})).unwrap();
        assert_eq!(dialog(&a), Some("fileChanged"));
        assert!(!a.quit_requested, "die App schließt noch nicht");
        assert!(inhalt(&path).contains("Neu von Mac A"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Skripte und Agenten bekommen nie einen Dialog, sondern einen Fehler.
    #[test]
    fn skripte_bekommen_einen_fehler() {
        let dir = ordner("skripte");
        let (mut a, path) = veraltet(&dir);
        let e = a.execute("file.save", json!({})).unwrap_err();
        assert!(wordcraft_engine::io::changed_elsewhere(&e), "{e}");
        assert_eq!(dialog(&a), None);
        assert!(inhalt(&path).contains("Neu von Mac A"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn eine_antwort_ohne_nachfrage_ist_ein_fehler() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        assert!(a.run("ui.fileChanged", json!({"answer": "overwrite"})).is_err());
        assert!(a.run("ui.fileChanged", json!({})).is_err());
    }

    /// Führt `tue` aus, nachdem das Fenster zur Ruhe gekommen ist, und sagt, ob es dabei sofort zum
    /// Neuzeichnen geweckt wurde.
    fn weckt_das_fenster(h: &mut egui_kittest::Harness<'_, WordApp>, tue: impl FnOnce(&mut WordApp)) -> bool {
        for _ in 0..10 {
            h.step();
        }
        let geweckt = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let merker = geweckt.clone();
        h.ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                merker.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        tue(h.state_mut());
        geweckt.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn fenster(a: WordApp) -> egui_kittest::Harness<'static, WordApp> {
        egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            a,
        )
    }

    /// Die Warnung wartet nicht auf die nächste Mausbewegung: Das Fenster wird von selbst geweckt.
    #[test]
    fn speichern_weckt_das_fenster_fuer_die_warnung() {
        let dir = ordner("wecken-speichern");
        let (a, _) = veraltet(&dir);
        let mut h = fenster(a);
        let geweckt = weckt_das_fenster(&mut h, |a| {
            a.run("file.save", json!({})).unwrap();
        });
        assert_eq!(dialog(h.state()), Some("fileChanged"));
        assert!(geweckt, "die Warnung muss ohne weitere Eingabe erscheinen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AutoSpeichern läuft auch bei verdecktem Fenster: Auch dann wird die Warnung gezeichnet.
    #[test]
    fn autosave_weckt_das_fenster_fuer_die_warnung() {
        let dir = ordner("wecken-autosave");
        let path = dir.join("a.docx");
        speichern_als(&path, "Alt");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("file.open", json!({"path": path.to_string_lossy()})).unwrap();
        a.run("file.save", json!({})).unwrap();
        von_aussen_ersetzen(&path, "Neu von Mac A");
        a.run("text.insert", json!({"text": "Mac B "})).unwrap();
        let mut h = fenster(a);
        let geweckt = weckt_das_fenster(&mut h, |a| a.autosave_tick(crate::clock::now_ms() + 10_000.0));
        assert_eq!(dialog(h.state()), Some("fileChanged"));
        assert!(geweckt, "die Warnung muss ohne weitere Eingabe erscheinen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Der Dialog im echten Fenster: Der Knopf ist da, hat den Fokus, und ein Klick überschreibt.
    #[test]
    fn der_dialog_laesst_sich_bedienen() {
        use egui_kittest::kittest::Queryable;
        let dir = ordner("fenster");
        let (a, path) = veraltet(&dir);
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            a,
        );
        for _ in 0..4 {
            h.step();
        }
        h.state_mut().run("file.save", json!({})).unwrap();
        for _ in 0..3 {
            h.step();
        }
        assert_eq!(dialog(h.state()), Some("fileChanged"));
        assert!(h.query_all_by_label("Save As…").any(|n| n.is_focused()), "der ungefährliche Knopf hat den Fokus");
        h.get_by_label("Overwrite").click();
        for _ in 0..3 {
            h.step();
        }
        assert_eq!(dialog(h.state()), None);
        assert!(inhalt(&path).contains("Mac B"));
        assert!(!h.state().session.dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
