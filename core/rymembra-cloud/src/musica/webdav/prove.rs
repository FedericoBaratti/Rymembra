//! Le prove del deposito WebDAV, contro un servitore finto su 127.0.0.1.

use super::*;
use crate::finto::{Finto, Vista, byte_finto, pezzo_di, risposta};
use crate::musica::cursore_scaduto;

/// Una risposta `multistatus` alla Nextcloud: prefisso `d:`, `href`
/// codificati, cartelle con `d:collection` vuoto.
fn multistatus(voci: &[(&str, bool, u64, &str)]) -> String {
    let mut corpo = String::from(
        r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">"#,
    );
    for (href, cartella, dimensione, etag) in voci {
        let tipo = if *cartella {
            "<d:resourcetype><d:collection/></d:resourcetype>"
        } else {
            "<d:resourcetype/>"
        };
        corpo.push_str(&format!(
            "<d:response><d:href>{href}</d:href><d:propstat><d:prop>{tipo}\
             <d:getcontentlength>{dimensione}</d:getcontentlength>\
             <d:getlastmodified>Tue, 01 Oct 2026 10:00:00 GMT</d:getlastmodified>\
             <d:getetag>&quot;{etag}&quot;</d:getetag></d:prop>\
             <d:status>HTTP/1.1 200 OK</d:status></d:propstat>\
             <d:propstat><d:prop><oc:size/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>\
             </d:response>"
        ));
    }
    corpo.push_str("</d:multistatus>");
    corpo
}

fn multi(corpo: &str) -> String {
    let mut testa = risposta(207, "application/xml", corpo.as_bytes());
    testa = testa.replacen("HTTP/1.1 207 Altro", "HTTP/1.1 207 Multi-Status", 1);
    testa
}

fn server(finto: &Finto) -> WebDav {
    WebDav::per_prova(
        finto.rete(),
        &format!("{}/remote.php/dav/files/mario/", finto.base),
        "mario",
        "segreta",
    )
}

#[test]
fn il_multistatus_si_legge_con_e_senza_prefissi() {
    let nextcloud = multistatus(&[
        ("/remote.php/dav/files/mario/Musica/", true, 0, "e1"),
        (
            "/remote.php/dav/files/mario/Musica/Live%20%7C%201977.flac",
            false,
            1234,
            "e2",
        ),
    ]);
    let risorse = leggi_multistatus(&nextcloud).expect("multistatus");
    assert_eq!(risorse.len(), 2);
    let file = risorse.get(1).expect("file");
    assert_eq!(
        file.percorso,
        "/remote.php/dav/files/mario/Musica/Live | 1977.flac"
    );
    assert_eq!((file.cartella, file.dimensione), (false, 1234));
    assert_eq!(
        file.etag.as_deref(),
        Some("e2"),
        "le virgolette dell'etag si tolgono"
    );
    assert!(risorse.first().is_some_and(|c| c.cartella));
    // Senza prefisso e con href assoluti, come pCloud e Koofr.
    let pcloud = r#"<?xml version="1.0"?><multistatus xmlns="DAV:"><response>
        <href>https://webdav.pcloud.com/Musica/a&amp;b.mp3</href>
        <propstat><prop><resourcetype/><getcontentlength>9</getcontentlength></prop>
        <status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
    let risorse = leggi_multistatus(pcloud).expect("pcloud");
    assert_eq!(
        risorse.first().map(|r| r.percorso.as_str()),
        Some("/Musica/a&b.mp3")
    );
}

#[test]
fn l_elenco_parte_dalla_catena_e_visita_le_cartelle() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        assert_eq!(vista.metodo, "PROPFIND");
        assert_eq!(vista.intestazione("depth"), Some("1"));
        assert_eq!(
            vista.intestazione("authorization"),
            Some(format!("Basic {}", base64(b"mario:segreta")).as_str())
        );
        match vista.percorso.as_str() {
            "/remote.php/dav/files/mario/" => multi(&multistatus(&[
                ("/remote.php/dav/files/mario/", true, 0, "c"),
                ("/remote.php/dav/files/mario/Musica/", true, 0, "m"),
                ("/remote.php/dav/files/mario/note.txt", false, 5, "n"),
            ])),
            "/remote.php/dav/files/mario/Musica/" => multi(&multistatus(&[
                ("/remote.php/dav/files/mario/Musica/", true, 0, "m"),
                (
                    "/remote.php/dav/files/mario/Musica/01.flac",
                    false,
                    100,
                    "f",
                ),
            ])),
            _ => risposta(404, "text/plain", b""),
        }
    });
    let mut voci = Vec::new();
    let completo = server(&finto)
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
            ("/remote.php/dav/files/mario", None),
            (
                "/remote.php/dav/files/mario/Musica",
                Some("/remote.php/dav/files/mario")
            ),
            (
                "/remote.php/dav/files/mario/Musica/01.flac",
                Some("/remote.php/dav/files/mario/Musica")
            ),
        ]
    );
    let brano = voci.last().expect("brano");
    assert!(brano.modificato_ms > 0);
    assert_eq!(brano.versione.as_deref(), Some("f"));
}

/// Le pagine che una prova dell'elenco accetta prima di dire basta: con un
/// elenco che non finisce, la prova fallisce invece di girare per sempre.
const PAGINE_AL_PIU: usize = 200;

/// L'elenco di `cima`, fermato dopo [`PAGINE_AL_PIU`] pagine: le voci, le
/// pagine e quel che `elenca` ha risposto.
fn elenco_con_un_freno(server: &WebDav, cima: &str) -> (Vec<VoceRemota>, usize, bool) {
    let mut voci = Vec::new();
    let mut pagine = 0;
    let completo = server
        .elenca(cima, &mut |pagina| {
            voci.extend(pagina);
            pagine += 1;
            if pagine >= PAGINE_AL_PIU {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("elenco");
    (voci, pagine, completo)
}

#[test]
fn un_href_antenato_non_fa_girare_l_elenco_per_sempre() {
    // Un server che, fra i figli di `Musica`, mette la cima del server, un
    // `..`, una cartella accanto e la stessa sottocartella due volte; e fra i
    // figli di `Musica/a` di nuovo `Musica`. Prima l'elenco riprendeva da `/`
    // e da `Musica` a ogni giro, e non finiva più.
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/remote.php/dav/files/mario/Musica/" => multi(&multistatus(&[
            ("/remote.php/dav/files/mario/Musica/", true, 0, "m"),
            ("/remote.php/dav/files/mario/Musica/a/", true, 0, "a"),
            ("/remote.php/dav/files/mario/Musica/a/", true, 0, "a"),
            ("/remote.php/dav/files/mario/Musica//a/", true, 0, "a"),
            ("/", true, 0, "r"),
            ("/remote.php/dav/files/mario/Musica/../", true, 0, "p"),
            ("/remote.php/dav/files/mario/Musica/%2E%2E/", true, 0, "p"),
            ("/remote.php/dav/files/mario/Altro/", true, 0, "x"),
            ("/remote.php/dav/files/mario/Musica/01.flac", false, 10, "f"),
        ])),
        "/remote.php/dav/files/mario/Musica/a/" => multi(&multistatus(&[
            ("/remote.php/dav/files/mario/Musica/a/", true, 0, "a"),
            ("/remote.php/dav/files/mario/Musica/", true, 0, "m"),
            (
                "/remote.php/dav/files/mario/Musica/a/02.flac",
                false,
                10,
                "g",
            ),
        ])),
        _ => multi(&multistatus(&[
            ("/", true, 0, "r"),
            ("/remote.php/dav/files/mario/Musica/", true, 0, "m"),
        ])),
    });
    let server = server(&finto);
    let (voci, pagine, completo) =
        elenco_con_un_freno(&server, "/remote.php/dav/files/mario/Musica");
    assert!(
        completo,
        "l'elenco arriva in fondo da sé, non perché la prova lo ferma: {pagine} pagine"
    );
    let chieste: Vec<String> = finto.viste().into_iter().map(|v| v.percorso).collect();
    assert_eq!(
        chieste,
        [
            "/remote.php/dav/files/mario/Musica/",
            "/remote.php/dav/files/mario/Musica/a/"
        ],
        "solo la cartella scelta e quel che le sta sotto, una volta ciascuna"
    );
    let mut ids: Vec<&str> = voci
        .iter()
        .filter(|v| v.genitore.as_deref() == Some("/remote.php/dav/files/mario/Musica"))
        .map(|v| v.id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "/remote.php/dav/files/mario/Musica/01.flac",
            "/remote.php/dav/files/mario/Musica/a"
        ]
    );
    assert!(
        voci.iter()
            .all(|v| v.id != "/" && v.nome != ".." && !v.id.contains("Altro")),
        "{voci:?}"
    );
    assert_eq!(
        server.tetto_raggiunto(),
        None,
        "un albero finito non tocca il tetto"
    );
}

#[test]
fn un_anello_che_allunga_il_percorso_si_ferma_al_tetto() {
    // Un collegamento simbolico a un antenato (`rclone serve webdav -L`,
    // Apache con `FollowSymLinks`): ogni cartella contiene `giro`, che la
    // ripete. Ogni percorso è nuovo, e il registro delle visitate non basta:
    // ci vuole il tetto di profondità.
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        let cartella = vista.percorso.as_str();
        let giro = format!("{cartella}giro/");
        let brano = format!("{cartella}01.flac");
        multi(&multistatus(&[
            (cartella, true, 0, "c"),
            (giro.as_str(), true, 0, "g"),
            (brano.as_str(), false, 10, "f"),
        ]))
    });
    let server = server(&finto);
    let (voci, pagine, completo) =
        elenco_con_un_freno(&server, "/remote.php/dav/files/mario/Musica");
    assert!(
        completo,
        "al tetto l'elenco si chiude da sé, e la radice non resta da rielencare a ogni giro: \
         {pagine} pagine"
    );
    assert_eq!(finto.viste().len(), PROFONDITA_ELENCO);
    assert_eq!(pagine, PROFONDITA_ELENCO);
    let in_fondo = voci
        .iter()
        .filter(|v| v.cartella)
        .map(|v| v.id.matches("/giro").count())
        .max();
    assert_eq!(
        in_fondo,
        Some(PROFONDITA_ELENCO),
        "le cartelle dell'ultimo livello ci sono, vuote"
    );
    assert_eq!(
        server.tetto_raggiunto(),
        Some(PROFONDITA_ELENCO),
        "il tetto si dice, a chi scrive il diario"
    );
    assert_eq!(server.tetto_raggiunto(), None, "e si dice una volta");
}

#[test]
fn una_password_sbagliata_si_dice() {
    let finto = Finto::avvia(|_: &Vista, _: &str| risposta(401, "text/plain", b""));
    let errore = server(&finto).chi().expect_err("rifiutato");
    assert_eq!(
        errore.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::CloudWrongCredentials
    );
}

#[test]
fn http_non_si_accetta_fuori_dalle_prove() {
    // Dentro le prove `http` passa (il servitore finto non ha certificati):
    // qui si prova la regola che lo decide.
    assert!(scomponi("http://nas.local/dav").is_ok());
    assert!(scomponi("ftp://x/y").is_err());
    let (origine, base) = scomponi("https://cloud.it/remote.php/dav/files/m%20r/").expect("ok");
    assert_eq!(
        (origine.as_str(), base.as_str()),
        ("https://cloud.it", "/remote.php/dav/files/m r")
    );
    assert_eq!(scomponi("https://cloud.it").expect("ok").1, "/");
}

#[test]
fn un_indirizzo_con_utente_e_password_non_si_accetta() {
    // La forma che molte guide mostrano: utente e password dentro
    // l'indirizzo. Tenuta, la password finiva nel nome del conto, nel
    // database coi dettagli del conto, e nel diario con ogni errore di rete.
    for indirizzo in [
        "https://mario:segreta@cloud.it/dav",
        "https://mario@cloud.it/dav",
        "https://mario:segreta@cloud.it",
    ] {
        let err = scomponi(indirizzo).expect_err(indirizzo);
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::CloudCredentialsInAddress,
            "{indirizzo}"
        );
        assert!(
            WebDav::nuovo(indirizzo, "mario", "segreta").is_err(),
            "{indirizzo}"
        );
    }
    // Una chiocciola nel **percorso** è un nome utente fatto da un'email,
    // come Nextcloud li mette nell'indirizzo WebDAV: quella va bene.
    let (origine, base) =
        scomponi("https://cloud.it/remote.php/dav/files/mario@esempio.it/").expect("ok");
    assert_eq!(
        (origine.as_str(), base.as_str()),
        ("https://cloud.it", "/remote.php/dav/files/mario@esempio.it")
    );
}

#[test]
fn un_etag_cambiato_vuol_dire_rielencare() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| {
        assert_eq!(vista.intestazione("depth"), Some("0"));
        multi(&multistatus(&[(
            "/remote.php/dav/files/mario/",
            true,
            0,
            "nuovo",
        )]))
    });
    let server = server(&finto);
    let adesso = rymembra_oauth::adesso_ms();
    let uguale = format!("nuovo|{adesso}");
    let (cambi, cursore) = server
        .cambiamenti(&uguale, Duration::ZERO)
        .expect("niente di nuovo");
    assert!(cambi.is_empty());
    assert_eq!(cursore, uguale);
    let diverso = server
        .cambiamenti(&format!("vecchio|{adesso}"), Duration::ZERO)
        .expect_err("cambiato");
    assert!(cursore_scaduto(&diverso));
    let scaduto = server
        .cambiamenti("nuovo|0", Duration::ZERO)
        .expect_err("ripasso");
    assert!(cursore_scaduto(&scaduto));
}

#[test]
fn una_cima_senza_etag_si_rielenca_solo_al_ripasso() {
    // Un server che l'`ETag` delle cartelle non lo dà: due `ETag` vuoti sono
    // uguali, e si rielenca al ripasso. Prima un `ETag` vuoto valeva
    // «cambiato», e si rielencava tutto a ogni giro.
    let finto = Finto::avvia(|_: &Vista, _: &str| {
        multi(
            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response>
            <d:href>/remote.php/dav/files/mario/</d:href><d:propstat><d:prop>
            <d:resourcetype><d:collection/></d:resourcetype></d:prop>
            <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#,
        )
    });
    let server = server(&finto);
    let primo = server.cursore_adesso().expect("cursore");
    assert!(primo.starts_with('|'), "{primo}");
    let (cambi, cursore) = server
        .cambiamenti(&primo, Duration::ZERO)
        .expect("niente di nuovo");
    assert!(cambi.is_empty());
    assert_eq!(cursore, primo);
    let ripasso = server
        .cambiamenti("|0", Duration::ZERO)
        .expect_err("è ora del ripasso");
    assert!(cursore_scaduto(&ripasso));
    assert_eq!(ripasso.cause(), Some("ripasso periodico"));
}

#[test]
fn la_cima_del_server_ha_una_barra_sola() {
    let server = WebDav::nuovo("https://cloud.it", "mario", "segreta").expect("indirizzo");
    assert_eq!(server.url("/"), "https://cloud.it/");
    assert_eq!(server.url(""), "https://cloud.it/");
    assert_eq!(server.url("/Musica"), "https://cloud.it/Musica/");
    assert_eq!(
        server.url("/Mus ica/Rock"),
        "https://cloud.it/Mus%20ica/Rock/"
    );
}

#[test]
fn spento_non_chiede_piu_niente() {
    // Scollegato mentre elencava: le domande dopo non partono, e lo specchio
    // del conto che non c'è più non si riempie.
    let finto = Finto::avvia(|_: &Vista, _: &str| {
        multi(&multistatus(&[(
            "/remote.php/dav/files/mario/",
            true,
            0,
            "c",
        )]))
    });
    let server = server(&finto);
    server.chi().expect("acceso");
    let prima = finto.viste().len();
    server.spegni();
    let errore = server
        .elenca("root", &mut |_| ControlFlow::Continue(()))
        .expect_err("spento");
    assert_eq!(
        errore.code().kind(),
        rymembra_domain::errors::ErrorCodeKind::CloudNotConnected
    );
    assert!(server.cursore_adesso().is_err());
    assert_eq!(
        finto.viste().len(),
        prima,
        "nessuna domanda dopo lo spegnimento"
    );
}

#[test]
fn il_brano_si_legge_a_pezzi_e_un_server_senza_range_si_legge_di_fila() {
    let finto = Finto::avvia(|vista: &Vista, _: &str| match vista.percorso.as_str() {
        "/remote.php/dav/files/mario/a.flac" => pezzo_di(vista, 40),
        "/remote.php/dav/files/mario/senza.flac" => risposta(
            200,
            "audio/flac",
            &(0..40).map(byte_finto).collect::<Vec<_>>(),
        ),
        _ => risposta(404, "text/plain", b""),
    });
    let server = server(&finto);
    let voce = |nome: &str| VoceRemota {
        id: format!("/remote.php/dav/files/mario/{nome}"),
        genitore: Some("/remote.php/dav/files/mario".to_owned()),
        nome: nome.to_owned(),
        cartella: false,
        dimensione: 40,
        modificato_ms: 0,
        versione: None,
        tag: None,
    };
    let pezzo = server
        .sorgente(&voce("a.flac"))
        .expect("sorgente")
        .pezzo(3, 4)
        .expect("pezzo");
    assert_eq!(pezzo.byte, (3..7).map(byte_finto).collect::<Vec<_>>());

    // Il server ignora `Range`: il brano si ascolta lo stesso, di fila.
    let senza = server.sorgente(&voce("senza.flac")).expect("sorgente");
    let byte = |da: u64, fino: u64| (da..fino).map(byte_finto).collect::<Vec<_>>();
    let pezzo = senza.pezzo(10, 4).expect("di fila");
    assert_eq!(pezzo.byte, byte(10, 14));
    assert_eq!(pezzo.totale, Some(40));
    let domande = || {
        finto
            .viste_a("/remote.php/dav/files/mario/senza.flac")
            .len()
    };
    // La domanda col `Range` ignorato, poi quella di fila.
    assert_eq!(domande(), 2);
    assert_eq!(senza.pezzo(14, 6).expect("avanti").byte, byte(14, 20));
    assert_eq!(
        senza.pezzo(30, 4).expect("un salto avanti").byte,
        byte(30, 34)
    );
    assert_eq!(domande(), 2, "avanti si legge dalla stessa risposta");
    assert_eq!(senza.pezzo(2, 3).expect("indietro").byte, byte(2, 5));
    assert_eq!(domande(), 3, "un salto indietro la riapre");
    assert!(
        senza.pezzo(40, 4).expect("in fondo").byte.is_empty(),
        "dopo la fine, niente"
    );
}

#[test]
fn le_date_http_e_il_base64() {
    assert_eq!(
        data_http("Tue, 01 Oct 2026 10:00:00 GMT"),
        rymembra_domain::tempo::istante_ms("2026-10-01T10:00:00Z")
    );
    assert_eq!(data_http("ieri"), None);
    assert_eq!(base64(b"mario:segreta"), "bWFyaW86c2VncmV0YQ==");
    assert_eq!(base64(b"a"), "YQ==");
    assert_eq!(base64(b"ab"), "YWI=");
    assert_eq!(base64(b"abc"), "YWJj");
    assert_eq!(unisci("/", "Musica"), "/Musica");
    assert_eq!(percorso_di_href("https://h/a%20b/"), "/a b");
}

#[test]
fn l_indirizzo_web_di_nextcloud() {
    let server = WebDav::per_prova(
        Rete::nuova("webdav", Duration::from_secs(1)),
        "https://cloud.it/remote.php/dav/files/mario/",
        "mario",
        "x",
    );
    let brano = VoceRemota {
        id: "/remote.php/dav/files/mario/Musica/01.flac".to_owned(),
        genitore: Some("/remote.php/dav/files/mario/Musica".to_owned()),
        nome: "01.flac".to_owned(),
        cartella: false,
        dimensione: 1,
        modificato_ms: 0,
        versione: None,
        tag: None,
    };
    assert_eq!(
        server.indirizzo_web(&brano).as_deref(),
        Some("https://cloud.it/index.php/apps/files/?dir=%2FMusica")
    );
}
