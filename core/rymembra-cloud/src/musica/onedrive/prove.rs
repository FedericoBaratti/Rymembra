//! Le prove di OneDrive, contro un Graph finto su questa macchina.

use std::sync::atomic::{AtomicUsize, Ordering};

use rymembra_net::flusso::FlussoHttp;

use super::*;
use crate::finto::{Finto, Vista, byte_finto, json, pezzo_di, rimando, risposta};
use crate::musica::cursore_scaduto;
use crate::oauth::{Credenziali, FORNITORE_MICROSOFT, Fornitore, Token};
use crate::portachiavi::{InMemoria, Portachiavi};

/// Un OneDrive collegato a un Graph finto, col token `a-0` ancora buono e un
/// punto di scambio finto che dà `a-1`, `a-2`… a ogni rinfresco.
fn collegato(finto: &Finto) -> OneDrive {
    let scambio: &'static str = format!("{}/token", finto.base).leak();
    let fornitore = Fornitore {
        scambio,
        ..FORNITORE_MICROSOFT
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
    OneDrive::per_prova(
        finto.rete(),
        &format!("{}/v1.0", finto.base),
        Arc::new(accesso),
    )
}

/// La risposta del punto di scambio finto: il token numero `n`.
fn token_numero(n: usize) -> String {
    json(
        200,
        &serde_json::json!({"access_token": format!("a-{n}"), "expires_in": 3600}),
    )
}

/// Un errore di Graph, com'è fatto.
fn errore_graph(stato: u16, codice: &str) -> String {
    json(
        stato,
        &serde_json::json!({"error": {"code": codice, "message": "messaggio di Graph"}}),
    )
}

#[test]
fn chi_e_l_id_del_drive_e_l_email_dell_utente() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso.starts_with("/v1.0/me/drive?") {
            json(
                200,
                &serde_json::json!({
                    "id": "d3e1f2a3b4c5",
                    "driveType": "personal",
                    "owner": {"user": {"displayName": "Mario"}}
                }),
            )
        } else if vista.percorso.starts_with("/v1.0/me?") {
            json(
                200,
                &serde_json::json!({
                    "displayName": "Mario Rossi",
                    "mail": null,
                    "userPrincipalName": "mario@outlook.com"
                }),
            )
        } else {
            errore_graph(404, "itemNotFound")
        }
    });
    let onedrive = collegato(&finto);
    let chi = onedrive.chi().expect("identità");
    assert_eq!(
        chi,
        Identita {
            id_stabile: "d3e1f2a3b4c5".to_owned(),
            nome: "mario@outlook.com".to_owned(),
        }
    );
    assert!(
        finto
            .viste()
            .iter()
            .all(|v| v.intestazione("authorization") == Some("Bearer a-0")),
        "a Graph col token"
    );
    assert_eq!(onedrive.servizio(), Servizio::OneDrive);
    assert_eq!(
        onedrive.limiti(),
        crate::musica::limiti_di(Servizio::OneDrive)
    );
}

#[test]
fn senza_me_il_nome_viene_dal_proprietario_del_drive() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista.percorso.starts_with("/v1.0/me/drive?") {
            json(
                200,
                &serde_json::json!({"id": "d1", "owner": {"user": {"displayName": "Mario"}}}),
            )
        } else {
            errore_graph(403, "accessDenied")
        }
    });
    let chi = collegato(&finto).chi().expect("identità");
    assert_eq!(
        chi.nome, "Mario",
        "il nome è un ornamento: non fa fallire niente"
    );
}

#[test]
fn le_cartelle_del_selettore_sono_solo_cartelle_e_a_pagine() {
    let finto = Finto::avvia(|vista: &Vista, base: &str| {
        match vista.percorso.split('?').next().unwrap_or_default() {
            "/v1.0/me/drive/root/children" => json(
                200,
                &serde_json::json!({
                    "value": [
                        {"id": "A", "name": "Musica", "folder": {"childCount": 3}, "parentReference": {"id": "R"}},
                        {"id": "x", "name": "nota.txt", "file": {}, "size": 10, "parentReference": {"id": "R"}}
                    ],
                    "@odata.nextLink": format!("{base}/v1.0/pagina2")
                }),
            ),
            "/v1.0/pagina2" => json(
                200,
                &serde_json::json!({
                    "value": [{"id": "B", "name": "Podcast", "folder": {}, "parentReference": {"id": "R"}}]
                }),
            ),
            _ => errore_graph(404, "itemNotFound"),
        }
    });
    let cartelle = collegato(&finto).cartelle(None).expect("cartelle");
    let nomi: Vec<&str> = cartelle.iter().map(|c| c.nome.as_str()).collect();
    assert_eq!(nomi, ["Musica", "Podcast"]);
    assert!(cartelle.iter().all(|c| c.cartella && c.dimensione == 0));
    let viste = finto.viste();
    assert!(
        viste[0].percorso.contains("$top=200"),
        "{}",
        viste[0].percorso
    );
    assert_eq!(
        viste[1].intestazione("authorization"),
        Some("Bearer a-0"),
        "il nextLink è di Graph: il token lo segue"
    );
}

/// Un drive finto: radice `R` › `Musica` (`M`) › `Rock` (`K`) › `Live` (`L`).
///
/// In `Rock` un FLAC con la faccetta `audio` e una foto; in `Live` un MP3.
fn drive_finto() -> Finto {
    Finto::avvia(move |vista: &Vista, _: &str| {
        match vista.percorso.split('?').next().unwrap_or_default() {
            "/v1.0/me/drive/items/K" => json(
                200,
                &serde_json::json!({"id": "K", "name": "Rock", "folder": {}, "parentReference": {"id": "M"}}),
            ),
            "/v1.0/me/drive/items/M" => json(
                200,
                &serde_json::json!({"id": "M", "name": "Musica", "folder": {}, "parentReference": {"id": "R"}}),
            ),
            "/v1.0/me/drive/items/R" => json(
                200,
                &serde_json::json!({"id": "R", "name": "root", "folder": {}, "root": {}, "parentReference": {"driveId": "d1"}}),
            ),
            "/v1.0/me/drive/items/K/children" => json(
                200,
                &serde_json::json!({"value": [
                    {"id": "L", "name": "Live", "folder": {"childCount": 1}, "parentReference": {"id": "K"},
                     "lastModifiedDateTime": "2024-01-15T10:30:00Z", "eTag": "\"{L},1\""},
                    {"id": "F1", "name": "01 Intro.flac", "file": {"mimeType": "audio/flac"}, "size": 31_457_280,
                     "parentReference": {"id": "K"}, "lastModifiedDateTime": "2024-01-15T10:30:00.123Z",
                     "eTag": "\"{F1},4\"", "cTag": "\"c:{F1},2\"",
                     "audio": {"title": "Intro", "artist": "Gruppo", "album": "Disco", "albumArtist": "Gruppo",
                               "genre": "Rock", "year": 1977, "track": 1, "disc": 1, "duration": 215_000,
                               "bitrate": 900, "hasDrm": false}},
                    {"id": "P1", "name": "copertina.jpg", "file": {}, "size": 100, "parentReference": {"id": "K"}}
                ]}),
            ),
            "/v1.0/me/drive/items/L/children" => json(
                200,
                &serde_json::json!({"value": [
                    {"id": "F2", "name": "02 Live.mp3", "file": {}, "size": 5_000_000, "parentReference": {"id": "L"},
                     "eTag": "\"{F2},1\"", "audio": {}}
                ]}),
            ),
            _ => errore_graph(404, "itemNotFound"),
        }
    })
}

#[test]
fn l_elenco_comincia_dagli_antenati_e_scende_per_cartelle() {
    let finto = drive_finto();
    let onedrive = collegato(&finto);
    let mut pagine: Vec<Vec<VoceRemota>> = Vec::new();
    let completo = onedrive
        .elenca("K", &mut |voci| {
            pagine.push(voci);
            ControlFlow::Continue(())
        })
        .expect("elenco");
    assert!(completo, "arrivato in fondo");
    assert_eq!(pagine.len(), 2, "una pagina per cartella");

    let prima: Vec<(&str, Option<&str>, &str)> = pagine[0]
        .iter()
        .map(|v| (v.id.as_str(), v.genitore.as_deref(), v.nome.as_str()))
        .collect();
    assert_eq!(
        prima,
        [
            ("R", None, ""),
            ("M", Some("R"), "Musica"),
            ("K", Some("M"), "Rock"),
            ("L", Some("K"), "Live"),
            ("F1", Some("K"), "01 Intro.flac"),
        ],
        "la cima del drive ha nome vuoto e nessun genitore; la foto non interessa"
    );

    let flac = &pagine[0][4];
    assert!(!flac.cartella);
    assert_eq!(flac.dimensione, 31_457_280);
    assert_eq!(
        flac.versione.as_deref(),
        Some("\"c:{F1},2\""),
        "il cTag, che cambia solo col contenuto"
    );
    assert_eq!(flac.modificato_ms, 1_705_314_600_000);
    let tag = flac.tag.as_ref().expect("la faccetta audio");
    assert_eq!(tag.titolo.as_deref(), Some("Intro"));
    assert_eq!(tag.artista.as_deref(), Some("Gruppo"));
    assert_eq!(tag.album.as_deref(), Some("Disco"));
    assert_eq!(tag.artista_album.as_deref(), Some("Gruppo"));
    assert_eq!(tag.genere.as_deref(), Some("Rock"));
    assert_eq!(tag.anno, Some(1977));
    assert_eq!(tag.traccia, Some(1));
    assert_eq!(tag.disco, Some(1));
    assert_eq!(tag.durata_ms, Some(215_000));

    let live = &pagine[0][3];
    assert!(live.cartella);
    assert_eq!(
        live.versione.as_deref(),
        Some("\"{L},1\""),
        "senza cTag, l'eTag"
    );

    let mp3 = &pagine[1][0];
    assert_eq!(mp3.id, "F2");
    assert_eq!(mp3.tag, None, "una faccetta vuota non dice niente");

    let figli = finto.viste_a("/v1.0/me/drive/items/K/children");
    assert_eq!(figli.len(), 1);
    assert!(
        figli[0].percorso.contains(
            "$select=id,name,folder,file,size,lastModifiedDateTime,parentReference,eTag,cTag,audio,deleted"
        ),
        "{}",
        figli[0].percorso
    );
}

#[test]
fn un_elenco_fermato_non_e_completo() {
    let finto = drive_finto();
    let onedrive = collegato(&finto);
    let completo = onedrive
        .elenca("K", &mut |_| ControlFlow::Break(()))
        .expect("elenco");
    assert!(
        !completo,
        "fermato a metà: lo specchio non deve togliere niente"
    );
    assert!(
        finto.viste_a("/v1.0/me/drive/items/L/children").is_empty(),
        "dopo il basta non si chiede più niente"
    );
}

#[test]
fn una_pagina_senza_valori_e_un_guasto_e_non_un_vuoto() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista
            .percorso
            .starts_with("/v1.0/me/drive/items/K/children")
        {
            json(200, &serde_json::json!({"altro": 1}))
        } else if vista.percorso.starts_with("/v1.0/me/drive/items/K") {
            json(200, &serde_json::json!({"id": "K", "name": "", "root": {}}))
        } else {
            errore_graph(404, "itemNotFound")
        }
    });
    let onedrive = collegato(&finto);
    let esito = onedrive.elenca("K", &mut |_| ControlFlow::Continue(()));
    assert_eq!(
        esito.expect_err("un guasto").code().kind().code(),
        "net.badSchema",
        "presa per vuota, l'elenco finirebbe «completo» e lo specchio toglierebbe tutto"
    );
}

#[test]
fn un_429_aspetta_e_riprova() {
    let volte = Arc::new(AtomicUsize::new(0));
    let contatore = Arc::clone(&volte);
    let finto = Finto::avvia(move |_: &Vista, _: &str| {
        if contatore.fetch_add(1, Ordering::SeqCst) == 0 {
            return "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned();
        }
        json(200, &serde_json::json!({"value": []}))
    });
    let cartelle = collegato(&finto)
        .cartelle(None)
        .expect("la seconda volta passa");
    assert!(cartelle.is_empty());
    assert_eq!(volte.load(Ordering::SeqCst), 2);
}

#[test]
fn il_cursore_di_adesso_e_il_delta_link() {
    let finto = Finto::avvia(|vista: &Vista, base: &str| {
        if vista.percorso == "/v1.0/me/drive/root/delta?token=latest" {
            json(
                200,
                &serde_json::json!({
                    "value": [],
                    "@odata.deltaLink": format!("{base}/v1.0/me/drive/root/delta?token=T1")
                }),
            )
        } else {
            errore_graph(404, "itemNotFound")
        }
    });
    let cursore = collegato(&finto).cursore_adesso().expect("cursore");
    assert_eq!(
        cursore,
        format!("{}/v1.0/me/drive/root/delta?token=T1", finto.base)
    );
}

#[test]
fn i_cambiamenti_seguono_le_pagine_fino_al_delta_link() {
    let finto = Finto::avvia(|vista: &Vista, base: &str| match vista.percorso.as_str() {
        "/v1.0/me/drive/root/delta?token=T1" => json(
            200,
            &serde_json::json!({
                "value": [
                    {"id": "R", "name": "root", "folder": {}, "root": {}},
                    {"id": "F1", "name": "01 Intro.flac", "file": {}, "size": 10, "parentReference": {"id": "K"}, "cTag": "c2"},
                    {"id": "F9", "deleted": {"state": "deleted"}, "parentReference": {"id": "K"}}
                ],
                "@odata.nextLink": format!("{base}/v1.0/me/drive/root/delta?token=T1b")
            }),
        ),
        "/v1.0/me/drive/root/delta?token=T1b" => json(
            200,
            &serde_json::json!({
                "value": [
                    {"id": "F2", "name": "foto.jpg", "file": {}, "size": 10, "parentReference": {"id": "L"}}
                ],
                "@odata.deltaLink": format!("{base}/v1.0/me/drive/root/delta?token=T2")
            }),
        ),
        _ => errore_graph(404, "itemNotFound"),
    });
    let onedrive = collegato(&finto);
    let (cambi, nuovo) = onedrive
        .cambiamenti(
            &format!("{}/v1.0/me/drive/root/delta?token=T1", finto.base),
            Duration::from_secs(30),
        )
        .expect("cambiamenti");
    assert_eq!(
        nuovo,
        format!("{}/v1.0/me/drive/root/delta?token=T2", finto.base)
    );
    assert_eq!(cambi.len(), 4);
    assert!(matches!(&cambi[0], Cambio::Voce(v) if v.genitore.is_none() && v.nome.is_empty()));
    assert!(
        matches!(&cambi[1], Cambio::Voce(v) if v.id == "F1" && v.versione.as_deref() == Some("c2"))
    );
    assert_eq!(
        cambi[2],
        Cambio::Tolta {
            id: "F9".to_owned()
        }
    );
    assert!(
        matches!(&cambi[3], Cambio::Voce(v) if v.nome == "foto.jpg"),
        "i cambiamenti passano tutto: un rinomina in qualcosa che non interessa è una rimozione"
    );
}

#[test]
fn un_cursore_scaduto_si_riconosce() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/v1.0/me/drive/root/delta?token=VECCHIO" => errore_graph(410, "resyncRequired"),
        "/v1.0/me/drive/root/delta?token=ALTRO" => {
            errore_graph(400, "resyncChangesApplyDifferences")
        }
        _ => errore_graph(404, "itemNotFound"),
    });
    let onedrive = collegato(&finto);
    for token in ["VECCHIO", "ALTRO"] {
        let err = onedrive
            .cambiamenti(
                &format!("{}/v1.0/me/drive/root/delta?token={token}", finto.base),
                Duration::ZERO,
            )
            .expect_err("scaduto");
        assert!(cursore_scaduto(&err), "{token}: {err}");
        assert!(err.cause().is_some_and(|c| c.contains("resync")));
    }
    assert_eq!(finto.viste().len(), 2, "un cursore scaduto non si ritenta");
}

#[test]
fn un_cursore_che_non_viene_da_graph_non_riceve_il_token() {
    let finto = Finto::avvia(|_: &Vista, _: &str| errore_graph(404, "itemNotFound"));
    let onedrive = collegato(&finto);
    let err = onedrive
        .cambiamenti("https://altrove.esempio/delta?token=T", Duration::ZERO)
        .expect_err("non è di Graph");
    assert!(
        cursore_scaduto(&err),
        "si tratta come scaduto: si rifà l'elenco"
    );
    let err = onedrive
        .cambiamenti(
            "http://127.0.0.1:1/v1.0/me/drive/root/delta",
            Duration::ZERO,
        )
        .expect_err("stessa macchina, altra porta: un'altra origine");
    assert!(cursore_scaduto(&err));
    assert!(finto.viste().is_empty());
}

#[test]
fn un_next_link_che_porta_altrove_non_si_segue() {
    let finto = Finto::avvia(|_: &Vista, _: &str| {
        json(
            200,
            &serde_json::json!({
                "value": [],
                "@odata.nextLink": "https://altrove.esempio/v1.0/pagina2"
            }),
        )
    });
    let err = collegato(&finto).cartelle(None).expect_err("altrove");
    assert_eq!(err.code().kind().code(), "net.badSchema");
    assert_eq!(finto.viste().len(), 1, "il token non è partito per altrove");
}

#[test]
fn un_401_di_graph_rinfresca_il_token_una_volta() {
    let rinfreschi = Arc::new(AtomicUsize::new(0));
    let contati = Arc::clone(&rinfreschi);
    let finto = Finto::avvia(move |vista: &Vista, _: &str| {
        if vista.percorso == "/token" {
            return token_numero(contati.fetch_add(1, Ordering::SeqCst) + 1);
        }
        if vista.intestazione("authorization") == Some("Bearer a-0") {
            return errore_graph(401, "InvalidAuthenticationToken");
        }
        json(200, &serde_json::json!({"id": "d1"}))
    });
    let chi = collegato(&finto).chi().expect("dopo il rinfresco");
    assert_eq!(chi.id_stabile, "d1");
    assert_eq!(rinfreschi.load(Ordering::SeqCst), 1);
    let ultima = finto
        .viste_a("/v1.0/me/drive")
        .pop()
        .expect("una richiesta");
    assert_eq!(ultima.intestazione("authorization"), Some("Bearer a-1"));
}

#[test]
fn un_401_anche_dopo_il_rinfresco_e_un_consenso_scaduto() {
    let rinfreschi = Arc::new(AtomicUsize::new(0));
    let contati = Arc::clone(&rinfreschi);
    let finto = Finto::avvia(move |vista: &Vista, _: &str| {
        if vista.percorso == "/token" {
            return token_numero(contati.fetch_add(1, Ordering::SeqCst) + 1);
        }
        errore_graph(401, "InvalidAuthenticationToken")
    });
    let err = collegato(&finto).chi().expect_err("revocato");
    assert_eq!(
        err.code(),
        &ErrorCode::CloudAuthExpired {
            servizio: "OneDrive".to_owned()
        }
    );
    assert_eq!(rinfreschi.load(Ordering::SeqCst), 1, "una volta, non tre");
}

/// La lunghezza del brano finto.
const BRANO: u64 = 1_000_000;

/// Un Graph che a `/content` risponde col rimando a `/firmato/<n>`, un numero
/// nuovo a ogni richiesta; `/firmato/<n>` serve gli intervalli finché
/// `scaduto(n, quante)` non dà uno stato di errore.
fn graph_dei_brani(
    scaduto: impl Fn(usize, usize) -> Option<u16> + Send + Sync + 'static,
) -> (Finto, Arc<AtomicUsize>) {
    let risolti = Arc::new(AtomicUsize::new(0));
    let contatore = Arc::clone(&risolti);
    let servite = Arc::new(Mutex::new(std::collections::HashMap::<usize, usize>::new()));
    let finto = Finto::avvia(move |vista: &Vista, base: &str| {
        if vista.percorso == "/v1.0/me/drive/items/F1/content" {
            let n = contatore.fetch_add(1, Ordering::SeqCst) + 1;
            return rimando(&format!("{base}/firmato/{n}?tempauth=segreto{n}"));
        }
        if let Some(resto) = vista.percorso.strip_prefix("/firmato/") {
            let n: usize = resto
                .split('?')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let quante = {
                let mut servite = servite.lock().unwrap();
                let quante = servite.entry(n).or_insert(0);
                *quante += 1;
                *quante
            };
            if let Some(stato) = scaduto(n, quante) {
                return risposta(stato, "text/plain", b"scaduto");
            }
            return pezzo_di(vista, BRANO);
        }
        errore_graph(404, "itemNotFound")
    });
    (finto, risolti)
}

/// La voce del brano finto.
fn brano() -> VoceRemota {
    VoceRemota {
        id: "F1".to_owned(),
        genitore: Some("K".to_owned()),
        nome: "01 Intro.flac".to_owned(),
        cartella: false,
        dimensione: BRANO,
        modificato_ms: 0,
        versione: Some("c1".to_owned()),
        tag: None,
    }
}

#[test]
fn un_brano_si_legge_dall_indirizzo_firmato_senza_token() {
    let (finto, risolti) = graph_dei_brani(|_, _| None);
    let sorgente = collegato(&finto).sorgente(&brano()).expect("sorgente");
    let primo = sorgente.pezzo(0, 10).expect("primo pezzo");
    let secondo = sorgente.pezzo(500_000, 10).expect("secondo pezzo");
    assert_eq!(primo.byte, (0..10).map(byte_finto).collect::<Vec<u8>>());
    assert_eq!(
        secondo.byte,
        (500_000..500_010).map(byte_finto).collect::<Vec<u8>>()
    );
    assert_eq!(primo.totale, Some(BRANO));
    assert_eq!(
        primo.url_finale, None,
        "l'indirizzo firmato non esce da qui"
    );
    assert_eq!(risolti.load(Ordering::SeqCst), 1, "il rimando si ricorda");
    let contenuto = finto.viste_a("/v1.0/me/drive/items/F1/content");
    assert_eq!(
        contenuto[0].intestazione("authorization"),
        Some("Bearer a-0")
    );
    let firmati = finto.viste_a("/firmato/");
    assert_eq!(firmati.len(), 2);
    assert!(
        firmati
            .iter()
            .all(|v| v.intestazione("authorization").is_none()),
        "il permesso è nell'indirizzo: il token non va a chi non è Graph"
    );
    assert_eq!(
        firmati[1].intestazione("range"),
        Some("bytes=500000-500009")
    );
}

#[test]
fn un_indirizzo_firmato_che_scade_a_meta_brano_si_chiede_di_nuovo() {
    // Il primo indirizzo serve un pezzo e poi scade, come dopo un'ora di
    // ascolto; il secondo funziona.
    let (finto, risolti) = graph_dei_brani(|n, quante| (n == 1 && quante > 1).then_some(401));
    let onedrive = collegato(&finto);
    let mut flusso =
        FlussoHttp::da(onedrive.sorgente(&brano()).expect("sorgente")).expect("il brano si apre");
    let mut tutto = Vec::new();
    std::io::Read::read_to_end(&mut flusso, &mut tutto).expect("arriva in fondo");
    assert_eq!(tutto.len(), usize::try_from(BRANO).unwrap());
    assert!(
        tutto.iter().zip(0_u64..).all(|(b, n)| *b == byte_finto(n)),
        "i byte sono quelli, anche dopo il cambio d'indirizzo"
    );
    assert_eq!(
        risolti.load(Ordering::SeqCst),
        2,
        "risolto di nuovo una volta"
    );
}

#[test]
fn un_indirizzo_che_non_torna_buono_e_un_guasto_che_non_nomina_la_firma() {
    let (finto, risolti) = graph_dei_brani(|_, _| Some(403));
    let sorgente = collegato(&finto).sorgente(&brano()).expect("sorgente");
    let err = sorgente.pezzo(0, 10).expect_err("scaduto per sempre");
    assert_eq!(
        risolti.load(Ordering::SeqCst),
        2,
        "una volta sola, non un giro"
    );
    assert!(matches!(err.code(), ErrorCode::NetHttp { status: 403, .. }));
    let scritto = format!("{err:?} {err}");
    assert!(!scritto.contains("tempauth"), "{scritto}");
    assert!(!scritto.contains("/firmato/"), "{scritto}");
    assert!(scritto.contains("/items/F1/content"), "{scritto}");
}

#[test]
fn un_file_sparito_e_cloud_not_found() {
    let finto = Finto::avvia(|_: &Vista, _: &str| errore_graph(404, "itemNotFound"));
    let sorgente = collegato(&finto).sorgente(&brano()).expect("sorgente");
    let err = sorgente.pezzo(0, 10).expect_err("sparito");
    assert_eq!(err.code(), &ErrorCode::CloudNotFound);
}

#[test]
fn un_401_su_content_rinfresca_il_token_una_volta() {
    let rinfreschi = Arc::new(AtomicUsize::new(0));
    let contati = Arc::clone(&rinfreschi);
    let finto = Finto::avvia(move |vista: &Vista, base: &str| {
        if vista.percorso == "/token" {
            return token_numero(contati.fetch_add(1, Ordering::SeqCst) + 1);
        }
        if vista
            .percorso
            .starts_with("/v1.0/me/drive/items/F1/content")
        {
            if vista.intestazione("authorization") == Some("Bearer a-0") {
                return errore_graph(401, "InvalidAuthenticationToken");
            }
            return rimando(&format!("{base}/firmato/1"));
        }
        pezzo_di(vista, BRANO)
    });
    let sorgente = collegato(&finto).sorgente(&brano()).expect("sorgente");
    let pezzo = sorgente.pezzo(0, 4).expect("dopo il rinfresco");
    assert_eq!(pezzo.byte.len(), 4);
    assert_eq!(rinfreschi.load(Ordering::SeqCst), 1);
}

#[test]
fn se_graph_serve_sul_posto_si_legge_content_col_token() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        if vista
            .percorso
            .starts_with("/v1.0/me/drive/items/F1/content")
        {
            if vista.intestazione("range").is_some() {
                return pezzo_di(vista, BRANO);
            }
            return risposta(200, "audio/flac", b"tutto il brano");
        }
        errore_graph(404, "itemNotFound")
    });
    let sorgente = collegato(&finto).sorgente(&brano()).expect("sorgente");
    let pezzo = sorgente.pezzo(100, 10).expect("pezzo");
    assert_eq!(pezzo.byte, (100..110).map(byte_finto).collect::<Vec<u8>>());
    let viste = finto.viste_a("/v1.0/me/drive/items/F1/content");
    assert_eq!(viste.len(), 2, "la domanda «dove», poi l'intervallo");
    assert_eq!(viste[1].intestazione("authorization"), Some("Bearer a-0"));
    assert_eq!(viste[1].intestazione("range"), Some("bytes=100-109"));
}

#[test]
fn l_indirizzo_web_apre_la_cartella_su_onedrive() {
    let finto = Finto::avvia(|_: &Vista, _: &str| errore_graph(404, "itemNotFound"));
    let onedrive = collegato(&finto);
    let voce = VoceRemota {
        id: "D3E1F2A3B4C5!1234".to_owned(),
        genitore: Some("D3E1F2A3B4C5!100".to_owned()),
        ..brano()
    };
    assert_eq!(
        onedrive.indirizzo_web(&voce).as_deref(),
        Some("https://onedrive.live.com/?cid=d3e1f2a3b4c5&id=D3E1F2A3B4C5%21100")
    );
    let senza_forma = VoceRemota {
        id: "01ABCDEF".to_owned(),
        genitore: Some("01PADRE".to_owned()),
        ..brano()
    };
    assert_eq!(onedrive.indirizzo_web(&senza_forma), None);
}

#[test]
fn una_voce_senza_id_non_e_una_voce() {
    assert_eq!(voce(&serde_json::json!({"name": "x.mp3"})), None);
    let senza_data =
        voce(&serde_json::json!({"id": "x", "name": "x.mp3", "file": {}})).expect("una voce");
    assert_eq!(senza_data.modificato_ms, 0);
    assert_eq!(senza_data.versione, None);
    assert_eq!(senza_data.genitore, None);
}
