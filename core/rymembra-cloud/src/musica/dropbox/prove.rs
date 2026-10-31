//! Le prove del deposito Dropbox, contro un servitore finto su 127.0.0.1.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::finto::{Finto, Vista, byte_finto, json, pezzo_di};
use crate::musica::cursore_scaduto;
use crate::oauth::{Credenziali, FORNITORE_DROPBOX, Fornitore, Token};
use crate::portachiavi::{InMemoria, Portachiavi};

fn collegato(finto: &Finto) -> Dropbox {
    let scambio: &'static str = format!("{}/token", finto.base).leak();
    let fornitore = Fornitore {
        scambio,
        ..FORNITORE_DROPBOX
    };
    let accesso = Accesso::appena_collegato(
        finto.rete(),
        fornitore,
        Credenziali {
            client_id: "app".to_owned(),
            client_secret: None,
        },
        Token {
            access_token: "a-0".to_owned(),
            refresh_token: Some("r-0".to_owned()),
            scade_ms: rymembra_oauth::adesso_ms() + 3_600_000,
            email: None,
        },
        Arc::new(InMemoria::nuovo()) as Arc<dyn Portachiavi>,
    );
    Dropbox::per_prova(finto.rete(), &finto.base, Arc::new(accesso))
}

fn file(percorso: &str, nome: &str, dimensione: u64) -> Value {
    json!({
        ".tag": "file", "name": nome, "path_lower": percorso, "path_display": percorso,
        "id": format!("id:{percorso}"), "size": dimensione,
        "server_modified": "2026-10-01T10:00:00Z", "rev": "015f", "content_hash": "h1"
    })
}

fn cartella(percorso: &str, nome: &str) -> Value {
    json!({ ".tag": "folder", "name": nome, "path_lower": percorso, "path_display": percorso,
            "id": format!("id:{percorso}") })
}

#[test]
fn chi_e_l_account_id_e_l_email() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso == "/2/users/get_current_account" {
            json(
                200,
                &json!({"account_id": "dbid:AAA", "email": "mario@esempio.it",
                        "name": {"display_name": "Mario"}}),
            )
        } else {
            json(404, &json!({}))
        }
    });
    let dropbox = collegato(&finto);
    let chi = dropbox.chi().expect("identità");
    assert_eq!(
        chi,
        Identita {
            id_stabile: "dbid:AAA".to_owned(),
            nome: "mario@esempio.it".to_owned(),
        }
    );
    assert!(
        finto
            .viste()
            .iter()
            .all(|v| v.intestazione("authorization") == Some("Bearer a-0") && v.metodo == "POST")
    );
    assert_eq!(dropbox.servizio(), Servizio::Dropbox);
}

#[test]
fn l_elenco_della_cima_e_ricorsivo_a_pagine_e_i_genitori_vengono_dal_percorso() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/2/files/list_folder" => json(
            200,
            &json!({
                "entries": [
                    cartella("/musica", "Musica"),
                    file("/musica/01 intro.flac", "01 Intro.flac", 1000),
                    file("/musica/copertina.jpg", "copertina.jpg", 10),
                ],
                "cursor": "c1", "has_more": true
            }),
        ),
        "/2/files/list_folder/continue" => json(
            200,
            &json!({
                "entries": [file("/musica/rock/02.mp3", "02.mp3", 500)],
                "cursor": "c2", "has_more": false
            }),
        ),
        _ => json(404, &json!({})),
    });
    let mut pagine = Vec::new();
    let completo = collegato(&finto)
        .elenca(CIMA, &mut |voci| {
            pagine.push(voci);
            ControlFlow::Continue(())
        })
        .expect("elenco");
    assert!(completo);
    assert_eq!(pagine.len(), 2);
    let prima = pagine.first().expect("prima pagina");
    assert_eq!(
        prima.first().map(|v| (v.id.as_str(), v.genitore.clone())),
        Some((CIMA, None))
    );
    let ids: Vec<&str> = pagine.iter().flatten().map(|v| v.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            CIMA,
            "/musica",
            "/musica/01 intro.flac",
            "/musica/rock/02.mp3"
        ],
        "la copertina non interessa"
    );
    let intro = pagine
        .iter()
        .flatten()
        .find(|v| v.id == "/musica/01 intro.flac")
        .expect("il brano");
    assert_eq!(intro.genitore.as_deref(), Some("/musica"));
    assert_eq!(intro.nome, "01 Intro.flac");
    assert_eq!(intro.versione.as_deref(), Some("h1"));
    assert!(intro.modificato_ms > 0);
    let corpo = finto
        .viste_a("/2/files/list_folder")
        .into_iter()
        .find(|v| v.percorso == "/2/files/list_folder")
        .map(|v| v.corpo)
        .unwrap_or_default();
    assert!(corpo.contains("\"recursive\":true") && corpo.contains("\"path\":\"\""));
}

#[test]
fn una_cartella_scelta_porta_la_sua_catena_e_non_se_stessa_due_volte() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/2/files/get_metadata" => json(
            200,
            &json!({".tag": "folder", "name": "Rock", "path_lower": "/musica/rock",
                    "path_display": "/Musica/Rock", "id": "id:r"}),
        ),
        "/2/files/list_folder" => json(
            200,
            &json!({
                "entries": [cartella("/musica/rock", "Rock"), file("/musica/rock/a.flac", "a.flac", 9)],
                "cursor": "c", "has_more": false
            }),
        ),
        _ => json(404, &json!({})),
    });
    let mut voci = Vec::new();
    collegato(&finto)
        .elenca("/musica/rock", &mut |pagina| {
            voci.extend(pagina);
            ControlFlow::Continue(())
        })
        .expect("elenco");
    let catena: Vec<(&str, &str)> = voci
        .iter()
        .map(|v| (v.id.as_str(), v.nome.as_str()))
        .collect();
    assert_eq!(
        catena,
        [
            (CIMA, ""),
            ("/musica", "Musica"),
            ("/musica/rock", "Rock"),
            ("/musica/rock/a.flac", "a.flac")
        ]
    );
}

#[test]
fn i_cambiamenti_si_aspettano_e_le_cancellazioni_arrivano_col_percorso() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/notify/2/files/list_folder/longpoll" => json(200, &json!({"changes": true})),
        "/2/files/list_folder/continue" => json(
            200,
            &json!({
                "entries": [
                    {".tag": "deleted", "name": "vecchio.mp3", "path_lower": "/musica/vecchio.mp3"},
                    file("/musica/nuovo.flac", "nuovo.flac", 7),
                ],
                "cursor": "c-nuovo", "has_more": false
            }),
        ),
        _ => json(404, &json!({})),
    });
    let dropbox = collegato(&finto);
    let (cambi, cursore) = dropbox
        .cambiamenti("c-vecchio", Duration::from_secs(30))
        .expect("cambiamenti");
    assert_eq!(cursore, "c-nuovo");
    assert_eq!(cambi.len(), 2);
    assert!(matches!(cambi.first(), Some(Cambio::Tolta { id }) if id == "/musica/vecchio.mp3"));
    let attesa = finto.viste_a("/notify/2/files/list_folder/longpoll");
    assert!(
        attesa
            .iter()
            .all(|v| v.intestazione("authorization").is_none()),
        "il long-poll non vuole il token"
    );
}

#[test]
fn senza_cambiamenti_il_cursore_resta_quello() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/notify/2/files/list_folder/longpoll" => json(200, &json!({"changes": false})),
        _ => json(404, &json!({})),
    });
    let (cambi, cursore) = collegato(&finto)
        .cambiamenti("c", Duration::from_secs(30))
        .expect("cambiamenti");
    assert!(cambi.is_empty());
    assert_eq!(cursore, "c");
    assert!(finto.viste_a("/2/files/list_folder/continue").is_empty());
}

#[test]
fn un_cursore_azzerato_e_un_cursore_scaduto() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/notify/2/files/list_folder/longpoll" => json(200, &json!({"changes": true})),
        "/2/files/list_folder/continue" => json(
            409,
            &json!({"error_summary": "reset/..", "error": {".tag": "reset"}}),
        ),
        _ => json(404, &json!({})),
    });
    let errore = collegato(&finto)
        .cambiamenti("c", Duration::from_secs(30))
        .expect_err("azzerato");
    assert!(cursore_scaduto(&errore), "{errore:?}");
}

#[test]
fn un_percorso_che_non_c_e_e_cloud_not_found() {
    let finto = Finto::avvia(|_: &Vista, _: &str| {
        json(409, &json!({"error_summary": "path/not_found/.."}))
    });
    let errore = collegato(&finto)
        .cartelle(Some("/sparita"))
        .expect_err("non trovata");
    assert_eq!(
        errore.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::CloudNotFound
    );
}

#[test]
fn il_brano_si_legge_dal_collegamento_e_un_collegamento_scaduto_si_rinnova_una_volta() {
    let collegamenti = Arc::new(AtomicUsize::new(0));
    let contati = Arc::clone(&collegamenti);
    let finto = Finto::avvia(
        move |vista: &Vista, base: &str| match vista.percorso.as_str() {
            "/2/files/get_temporary_link" => {
                let n = contati.fetch_add(1, Ordering::SeqCst);
                json(
                    200,
                    &json!({"link": format!("{base}/link/{n}"), "metadata": {}}),
                )
            }
            // Il primo collegamento è già scaduto.
            "/link/0" => json(410, &json!({})),
            "/link/1" => pezzo_di(vista, 100),
            _ => json(404, &json!({})),
        },
    );
    let dropbox = collegato(&finto);
    let sorgente = dropbox
        .sorgente(&voce(&file("/a.flac", "a.flac", 100)).expect("voce"))
        .expect("sorgente");
    let pezzo = sorgente.pezzo(10, 5).expect("pezzo");
    assert_eq!(pezzo.byte, (10..15).map(byte_finto).collect::<Vec<_>>());
    assert_eq!(pezzo.url_finale, None, "il collegamento non esce");
    assert_eq!(collegamenti.load(Ordering::SeqCst), 2);
    assert!(
        finto
            .viste_a("/link/")
            .iter()
            .all(|v| v.intestazione("authorization").is_none()),
        "il collegamento si legge senza token"
    );
    // Un secondo pezzo riusa il collegamento buono.
    let _ = sorgente.pezzo(0, 5).expect("pezzo");
    assert_eq!(collegamenti.load(Ordering::SeqCst), 2);
}

#[test]
fn l_indirizzo_web_e_la_cartella_madre() {
    let finto = Finto::avvia(|_: &Vista, _: &str| json(404, &json!({})));
    let dropbox = collegato(&finto);
    let brano = voce(&file("/musica/rock & roll/01.mp3", "01.mp3", 1)).expect("voce");
    assert_eq!(
        dropbox.indirizzo_web(&brano).as_deref(),
        Some("https://www.dropbox.com/home/musica/rock%20%26%20roll")
    );
}

#[test]
fn la_madre_di_un_percorso() {
    assert_eq!(madre_di("/a.flac"), CIMA);
    assert_eq!(madre_di("/musica/a.flac"), "/musica");
    assert_eq!(percorso_di(CIMA), "");
    assert_eq!(percorso_di("/musica"), "/musica");
}
