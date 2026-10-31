//! Le prove del deposito Google Drive, contro un servitore finto su 127.0.0.1.

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::*;
use crate::finto::{Finto, Vista, byte_finto, json, pezzo_di};
use crate::musica::cursore_scaduto;
use crate::oauth::{Credenziali, FORNITORE_GOOGLE_MUSICA, Fornitore, Token};
use crate::portachiavi::{InMemoria, Portachiavi};

fn collegato(finto: &Finto) -> GoogleDrive {
    let scambio: &'static str = format!("{}/token", finto.base).leak();
    let fornitore = Fornitore {
        scambio,
        ..FORNITORE_GOOGLE_MUSICA
    };
    let accesso = Accesso::appena_collegato(
        finto.rete(),
        fornitore,
        Credenziali {
            client_id: "app".to_owned(),
            client_secret: Some("segreto".to_owned()),
        },
        Token {
            access_token: "a-0".to_owned(),
            refresh_token: Some("r-0".to_owned()),
            scade_ms: rymembra_oauth::adesso_ms() + 3_600_000,
            email: None,
        },
        Arc::new(InMemoria::nuovo()) as Arc<dyn Portachiavi>,
    );
    GoogleDrive::per_prova(finto.rete(), &finto.base, Arc::new(accesso))
}

fn file(id: &str, nome: &str, genitore: &str, tipo: &str) -> Value {
    json!({"id": id, "name": nome, "parents": [genitore], "mimeType": tipo,
           "size": "1000", "modifiedTime": "2026-10-01T10:00:00.000Z", "md5Checksum": format!("md5-{id}")})
}

/// La domanda `q` di una vista, decodificata.
fn domanda(vista: &Vista) -> String {
    vista.parametro("q").unwrap_or_default()
}

#[test]
fn chi_e_il_permission_id() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso.starts_with("/drive/v3/about?") {
            json(
                200,
                &json!({"user": {"permissionId": "0123", "emailAddress": "m@gmail.com"}}),
            )
        } else {
            json(404, &json!({}))
        }
    });
    let drive = collegato(&finto);
    assert_eq!(
        drive.chi().expect("identità"),
        Identita {
            id_stabile: "0123".to_owned(),
            nome: "m@gmail.com".to_owned()
        }
    );
    assert_eq!(drive.servizio(), Servizio::GoogleDrive);
}

#[test]
fn l_elenco_visita_le_cartelle_a_gruppi_e_salta_i_documenti_google() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        let percorso = vista.percorso.split('?').next().unwrap_or_default();
        match percorso {
            "/drive/v3/files/root" => json(
                200,
                &json!({"id": "R", "name": "My Drive", "mimeType": CARTELLA}),
            ),
            "/drive/v3/files" => {
                let q = domanda(vista);
                if q.contains("'R' in parents") {
                    json(
                        200,
                        &json!({"files": [
                            file("M", "Musica", "R", CARTELLA),
                            file("d", "Appunti", "R", "application/vnd.google-apps.document"),
                            file("f1", "01.flac", "R", "audio/flac"),
                        ]}),
                    )
                } else if q.contains("'M' in parents") {
                    json(
                        200,
                        &json!({"files": [file("f2", "02.mp3", "M", "audio/mpeg")]}),
                    )
                } else {
                    json(200, &json!({"files": []}))
                }
            }
            _ => json(404, &json!({})),
        }
    });
    let mut voci = Vec::new();
    let completo = collegato(&finto)
        .elenca("root", &mut |pagina| {
            voci.extend(pagina);
            ControlFlow::Continue(())
        })
        .expect("elenco");
    assert!(completo);
    let ids: Vec<(&str, Option<&str>)> = voci
        .iter()
        .map(|v| (v.id.as_str(), v.genitore.as_deref()))
        .collect();
    assert_eq!(
        ids,
        [
            ("R", None),
            ("M", Some("R")),
            ("f1", Some("R")),
            ("f2", Some("M"))
        ]
    );
    let cima = voci.first().expect("cima");
    assert_eq!(cima.nome, "", "la cima del drive non entra nel percorso");
    let f1 = voci.iter().find(|v| v.id == "f1").expect("f1");
    assert_eq!(
        (f1.dimensione, f1.versione.as_deref()),
        (1000, Some("md5-f1"))
    );
    assert!(f1.modificato_ms > 0);
}

#[test]
fn i_cambiamenti_dicono_tolte_e_cestinate_e_danno_il_segno_nuovo() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso.starts_with("/drive/v3/changes?") {
            json(
                200,
                &json!({
                    "changes": [
                        {"fileId": "a", "removed": true},
                        {"fileId": "b", "removed": false, "file": {"id": "b", "name": "b.mp3", "parents": ["M"], "mimeType": "audio/mpeg", "trashed": true}},
                        {"fileId": "c", "removed": false, "file": file("c", "c.flac", "M", "audio/flac")},
                    ],
                    "newStartPageToken": "s2"
                }),
            )
        } else {
            json(404, &json!({}))
        }
    });
    let (cambi, segno) = collegato(&finto)
        .cambiamenti("s1", Duration::ZERO)
        .expect("cambiamenti");
    assert_eq!(segno, "s2");
    assert!(matches!(cambi.first(), Some(Cambio::Tolta { id }) if id == "a"));
    assert!(matches!(cambi.get(1), Some(Cambio::Tolta { id }) if id == "b"));
    assert!(matches!(cambi.get(2), Some(Cambio::Voce(v)) if v.id == "c"));
}

#[test]
fn un_segno_sconosciuto_e_un_cursore_scaduto() {
    let finto = Finto::avvia(|_: &Vista, _: &str| {
        json(
            404,
            &json!({"error": {"errors": [{"reason": "notFound"}], "message": "x"}}),
        )
    });
    let errore = collegato(&finto)
        .cambiamenti("vecchio", Duration::ZERO)
        .expect_err("scaduto");
    assert!(cursore_scaduto(&errore), "{errore:?}");
}

#[test]
fn un_limite_di_frequenza_e_un_rallentamento() {
    let finto = Finto::avvia(|_: &Vista, _: &str| {
        json(
            403,
            &json!({"error": {"errors": [{"reason": "userRateLimitExceeded"}], "message": "piano"}}),
        )
    });
    let errore = collegato(&finto).cartelle(None).expect_err("limite");
    assert_eq!(
        errore.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::NetRateLimited
    );
}

#[test]
fn il_brano_si_legge_col_token_e_un_401_rinfresca_una_volta() {
    let tentativi = Arc::new(AtomicUsize::new(0));
    let contati = Arc::clone(&tentativi);
    let finto = Finto::avvia(move |vista: &Vista, _: &str| {
        if vista.percorso == "/token" {
            return json(200, &json!({"access_token": "a-1", "expires_in": 3600}));
        }
        if vista.percorso.starts_with("/drive/v3/files/f1?alt=media") {
            let n = contati.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return json(401, &json!({}));
            }
            assert_eq!(vista.intestazione("authorization"), Some("Bearer a-1"));
            return pezzo_di(vista, 50);
        }
        json(404, &json!({}))
    });
    let drive = collegato(&finto);
    let sorgente = drive
        .sorgente(&voce(&file("f1", "01.flac", "R", "audio/flac"), None).expect("voce"))
        .expect("sorgente");
    let pezzo = sorgente.pezzo(5, 5).expect("pezzo");
    assert_eq!(pezzo.byte, (5..10).map(byte_finto).collect::<Vec<_>>());
    assert_eq!(tentativi.load(Ordering::SeqCst), 2);
}

#[test]
fn un_file_con_due_genitori_tiene_quello_della_visita() {
    let item = json!({"id": "x", "name": "x.mp3", "parents": ["Fuori", "Dentro"], "mimeType": "audio/mpeg"});
    let visitate = HashSet::from(["Dentro".to_owned()]);
    assert_eq!(
        voce(&item, Some(&visitate))
            .and_then(|v| v.genitore)
            .as_deref(),
        Some("Dentro")
    );
    assert_eq!(virgolette("l'album"), "l\\'album");
}

#[test]
fn gli_indirizzi_web() {
    let finto = Finto::avvia(|_: &Vista, _: &str| json(404, &json!({})));
    let drive = collegato(&finto);
    let brano = voce(&file("f1", "01.flac", "R", "audio/flac"), None).expect("voce");
    assert_eq!(
        drive.indirizzo_web(&brano).as_deref(),
        Some("https://drive.google.com/file/d/f1/view")
    );
    assert_eq!(
        drive.indirizzo_web(&condivisi("R")).as_deref(),
        Some("https://drive.google.com/drive/shared-with-me")
    );
}

/// Una voce senza madre visibile: una cartella condivisa con l'account.
fn senza_madre(id: &str, nome: &str, tipo: &str) -> Value {
    json!({"id": id, "name": nome, "mimeType": tipo, "modifiedTime": "2026-10-01T10:00:00.000Z"})
}

/// La cima di «Il mio Drive» del servitore finto.
fn cima_finta() -> String {
    json(
        200,
        &json!({"id": "R", "name": "My Drive", "mimeType": CARTELLA}),
    )
}

#[test]
fn in_cima_al_selettore_ci_sono_i_condivisi_e_le_scorciatoie() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso.starts_with("/drive/v3/files?") {
            json(
                200,
                &json!({"files": [
                    file("M", "Musica", "R", CARTELLA),
                    {"id": "s1", "name": "Musica di Marco", "parents": ["R"], "mimeType": SCORCIATOIA,
                     "shortcutDetails": {"targetId": "T", "targetMimeType": CARTELLA}},
                    {"id": "s2", "name": "Un brano", "parents": ["R"], "mimeType": SCORCIATOIA,
                     "shortcutDetails": {"targetId": "F", "targetMimeType": "audio/mpeg"}},
                    {"id": "s3", "name": "Ancora Musica", "parents": ["R"], "mimeType": SCORCIATOIA,
                     "shortcutDetails": {"targetId": "M", "targetMimeType": CARTELLA}},
                    file("f1", "01.flac", "R", "audio/flac"),
                ]}),
            )
        } else {
            json(404, &json!({}))
        }
    });
    let drive = collegato(&finto);
    let cartelle = drive.cartelle(None).expect("cartelle");
    let viste: Vec<(&str, &str)> = cartelle
        .iter()
        .map(|v| (v.id.as_str(), v.nome.as_str()))
        .collect();
    assert_eq!(
        viste,
        [
            (CONDIVISI, NOME_CONDIVISI),
            ("M", "Musica"),
            ("T", "Musica di Marco")
        ],
        "la scorciatoia a una cartella si apre come il suo bersaglio, una volta sola; quella a un file no"
    );
    assert!(cartelle.iter().all(|v| v.cartella));
    assert!(e_virtuale(CONDIVISI));
    let richieste = finto.viste_a("/drive/v3/files");
    assert_eq!(
        richieste.len(),
        1,
        "la cartella virtuale non si chiede a Google"
    );
    let q = richieste.first().map(domanda).unwrap_or_default();
    assert!(q.contains("'root' in parents"), "{q}");
    assert!(q.contains(SCORCIATOIA), "{q}");
}

#[test]
fn dentro_i_condivisi_si_chiede_shared_with_me() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso.starts_with("/drive/v3/files?") {
            json(
                200,
                &json!({"files": [senza_madre("S", "Musica di Marco", CARTELLA)]}),
            )
        } else {
            json(404, &json!({}))
        }
    });
    let cartelle = collegato(&finto)
        .cartelle(Some(CONDIVISI))
        .expect("condivise");
    assert_eq!(
        cartelle.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
        ["S"]
    );
    let richieste = finto.viste();
    assert_eq!(richieste.len(), 1);
    let q = richieste.first().map(domanda).unwrap_or_default();
    assert!(q.starts_with("sharedWithMe = true"), "{q}");
    assert!(!q.contains(CONDIVISI), "{q}");
}

/// Un Drive finto: `S` è condivisa con l'account (senza madre visibile), `C`
/// sta dentro `S`, e `X` ha per madre `P`, che l'account non può leggere.
fn drive_condiviso(vista: &Vista, _: &str) -> String {
    let percorso = vista.percorso.split('?').next().unwrap_or_default();
    match percorso {
        "/drive/v3/files/root" => cima_finta(),
        "/drive/v3/files/S" => json(200, &senza_madre("S", "Musica di Marco", CARTELLA)),
        "/drive/v3/files/C" => json(200, &file("C", "Rock", "S", CARTELLA)),
        "/drive/v3/files/X" => json(200, &file("X", "Jazz", "P", CARTELLA)),
        "/drive/v3/files" => {
            let q = domanda(vista);
            if q.contains("'S' in parents") {
                json(
                    200,
                    &json!({"files": [
                        file("C", "Rock", "S", CARTELLA),
                        file("f1", "01.flac", "S", "audio/flac"),
                    ]}),
                )
            } else if q.contains("'C' in parents") {
                json(
                    200,
                    &json!({"files": [file("f2", "02.mp3", "C", "audio/mpeg")]}),
                )
            } else {
                json(200, &json!({"files": []}))
            }
        }
        _ => json(
            404,
            &json!({"error": {"errors": [{"reason": "notFound"}], "message": "File not found"}}),
        ),
    }
}

/// Tutto l'elenco di `cima`, che deve arrivare in fondo.
fn elenco_di(drive: &GoogleDrive, cima: &str) -> Vec<VoceRemota> {
    let mut voci = Vec::new();
    let completo = drive
        .elenca(cima, &mut |pagina| {
            voci.extend(pagina);
            ControlFlow::Continue(())
        })
        .expect("elenco");
    assert!(completo);
    voci
}

#[test]
fn una_cartella_condivisa_sta_sotto_la_cartella_virtuale() {
    let finto = Finto::avvia(drive_condiviso);
    let voci = elenco_di(&collegato(&finto), "S");
    let ids: Vec<(&str, Option<&str>, &str)> = voci
        .iter()
        .map(|v| (v.id.as_str(), v.genitore.as_deref(), v.nome.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            ("R", None, ""),
            (CONDIVISI, Some("R"), NOME_CONDIVISI),
            ("S", Some(CONDIVISI), "Musica di Marco"),
            ("C", Some("S"), "Rock"),
            ("f1", Some("S"), "01.flac"),
            ("f2", Some("C"), "02.mp3"),
        ],
        "la cima resta la sola senza madre, e la condivisa tiene il suo nome"
    );
}

#[test]
fn una_sottocartella_condivisa_risale_fin_dove_si_vede() {
    let finto = Finto::avvia(drive_condiviso);
    let drive = collegato(&finto);
    // La madre di `C` si vede: la catena passa da lei.
    let voci = elenco_di(&drive, "C");
    let ids: Vec<&str> = voci.iter().take(4).map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["R", CONDIVISI, "S", "C"]);
    // La madre di `X` no: la salita si ferma, senza errore.
    let voci = elenco_di(&drive, "X");
    let x = voci.iter().find(|v| v.id == "X").expect("X");
    assert_eq!(
        (x.genitore.as_deref(), x.nome.as_str()),
        (Some(CONDIVISI), "Jazz")
    );
    assert_eq!(
        finto.viste_a("/drive/v3/files/root").len(),
        1,
        "la cima del drive si chiede una volta sola"
    );
}

#[test]
fn la_cartella_scelta_che_non_c_e_resta_un_errore() {
    let finto = Finto::avvia(drive_condiviso);
    let drive = collegato(&finto);
    let errore = drive
        .elenca("sparita", &mut |_| ControlFlow::Continue(()))
        .expect_err("non c'è");
    assert_eq!(errore.code().kind(), ErrorCodeKind::CloudNotFound);
    // La cartella virtuale non si elenca, e non si chiede a Google.
    let prima = finto.viste().len();
    let errore = drive
        .elenca(CONDIVISI, &mut |_| ControlFlow::Continue(()))
        .expect_err("virtuale");
    assert_eq!(errore.code().kind(), ErrorCodeKind::CloudNotFound);
    assert_eq!(finto.viste().len(), prima);
}

/// Il Drive condiviso, con i cambiamenti di `X` (madre `P`, che non si legge),
/// di `C` (madre `S`, che si legge) e di un file dentro `C`.
fn drive_condiviso_che_cambia(vista: &Vista, _: &str) -> String {
    let percorso = vista.percorso.split('?').next().unwrap_or_default();
    if percorso == "/drive/v3/changes" {
        return json(
            200,
            &json!({
                "changes": [
                    {"fileId": "X", "removed": false, "file": file("X", "Jazz 2", "P", CARTELLA)},
                    {"fileId": "C", "removed": false, "file": file("C", "Rock", "S", CARTELLA)},
                    {"fileId": "f3", "removed": false, "file": file("f3", "03.flac", "C", "audio/flac")},
                ],
                "newStartPageToken": "s2"
            }),
        );
    }
    drive_condiviso(vista, "")
}

/// Le voci dei cambiamenti: `(id, madre)`.
fn madri_dei_cambi(cambi: &[Cambio]) -> Vec<(&str, Option<&str>)> {
    cambi
        .iter()
        .filter_map(|c| match c {
            Cambio::Voce(v) => Some((v.id.as_str(), v.genitore.as_deref())),
            Cambio::Tolta { .. } => None,
        })
        .collect()
}

#[test]
fn nei_cambiamenti_una_cartella_con_la_madre_invisibile_va_sotto_i_condivisi() {
    // L'elenco aveva messo `X` sotto la cartella virtuale, perché la sua madre
    // non si legge. Quando `X` cambia, Drive la consegna con la madre vera: se
    // restasse lì, `X` perderebbe il percorso con tutto il suo sottoalbero.
    let finto = Finto::avvia(drive_condiviso_che_cambia);
    let drive = collegato(&finto);
    for _ in 0..2 {
        let (cambi, _) = drive
            .cambiamenti("s1", Duration::ZERO)
            .expect("cambiamenti");
        assert_eq!(
            madri_dei_cambi(&cambi),
            [("X", Some(CONDIVISI)), ("C", Some("S")), ("f3", Some("C")),]
        );
    }
    assert_eq!(
        finto.viste_a("/drive/v3/files/P").len(),
        1,
        "la madre si chiede una volta sola"
    );
}

#[test]
fn dopo_l_elenco_la_madre_invisibile_non_si_richiede() {
    let finto = Finto::avvia(drive_condiviso_che_cambia);
    let drive = collegato(&finto);
    let _ = elenco_di(&drive, "X");
    let _ = elenco_di(&drive, "C");
    let chieste = finto.viste().len();
    let (cambi, _) = drive
        .cambiamenti("s1", Duration::ZERO)
        .expect("cambiamenti");
    assert_eq!(
        madri_dei_cambi(&cambi).first().copied(),
        Some(("X", Some(CONDIVISI)))
    );
    assert_eq!(
        finto.viste().len(),
        chieste + 1,
        "solo la domanda dei cambiamenti: le madri le sa già la salita"
    );
}

#[test]
fn nei_cambiamenti_solo_la_cima_resta_senza_madre() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        let percorso = vista.percorso.split('?').next().unwrap_or_default();
        match percorso {
            "/drive/v3/files/root" => cima_finta(),
            "/drive/v3/changes" => json(
                200,
                &json!({
                    "changes": [
                        {"fileId": "R", "removed": false, "file": {"id": "R", "name": "My Drive", "mimeType": CARTELLA}},
                        {"fileId": "S", "removed": false, "file": senza_madre("S", "Musica di Marco", CARTELLA)},
                        {"fileId": "c", "removed": false, "file": file("c", "c.flac", "S", "audio/flac")},
                    ],
                    "newStartPageToken": "s2"
                }),
            ),
            _ => json(404, &json!({})),
        }
    });
    let drive = collegato(&finto);
    for _ in 0..2 {
        let (cambi, _) = drive
            .cambiamenti("s1", Duration::ZERO)
            .expect("cambiamenti");
        let voci: Vec<(&str, Option<&str>, &str)> = cambi
            .iter()
            .filter_map(|c| match c {
                Cambio::Voce(v) => Some((v.id.as_str(), v.genitore.as_deref(), v.nome.as_str())),
                Cambio::Tolta { .. } => None,
            })
            .collect();
        assert_eq!(
            voci,
            [
                ("R", None, ""),
                ("S", Some(CONDIVISI), "Musica di Marco"),
                ("c", Some("S"), "c.flac"),
            ]
        );
    }
    assert_eq!(
        finto.viste_a("/drive/v3/files/root").len(),
        1,
        "la cima si chiede una volta, non a ogni giro"
    );
}
