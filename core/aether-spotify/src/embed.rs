//! Il ripiego che non ha bisogno di gettoni: la pagina del riquadro
//! incorporabile.
//!
//! Quando la stretta di mano non riesce — cifrari ruotati, impronte scadute,
//! `clienttoken` che non risponde — resta questa: `open.spotify.com/embed/...`
//! è una pagina pubblica che porta dentro di sé, in un `<script>`, il JSON con
//! cui disegna il riquadro. Dentro quel JSON ci sono titolo, copertina e
//! l'elenco dei brani.
//!
//! # Il limite, e perché va detto invece che nascosto
//!
//! Sulle playlist lunghe l'elenco arriva **troncato**: il riquadro ne mostra
//! una parte e non è fatto per darne di più. Il vecchio albero se ne accorgeva e
//! lo annotava in un commento; qui va più in là, perché una playlist di
//! trecento brani importata con centoquaranta e nessun avviso è il guasto
//! peggiore che questo sottosistema possa produrre. Quando si trova un totale
//! dichiarato lo si porta su, e chi mostra il risultato confronta.
//!
//! # Due forme, perché Spotify le ha cambiate
//!
//! Il vecchio albero cercava solo `__NEXT_DATA__`. Le distribuzioni più recenti
//! del riquadro mettono lo stesso JSON in uno `<script id="initial-state">`
//! codificato in base64. Si provano tutte e due: costa una funzione in più e
//! copre il periodo in cui convivono.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify::{SpotifyContent, SpotifySource, SpotifyTrack};
use aether_net::http::{Corpo, Metodo, Rete, Richiesta};
use serde_json::Value;

use crate::url::Riferimento;

/// Quanto si scende dentro il JSON cercando i nodi che servono.
const PROFONDITA_MASSIMA: usize = 10;

/// Scarica e interpreta la pagina del riquadro.
///
/// # Errori
///
/// `spotify.notPublic` su un 404, `spotify.resolveFailed` se la pagina non
/// contiene il JSON atteso.
pub fn risolvi(rete: &Rete, riferimento: &Riferimento) -> Result<SpotifyContent, AppError> {
    let url = riferimento.url_embed();
    let risposta = rete.esegui(Richiesta {
        metodo: Metodo::Get,
        url: &url,
        intestazioni: &[("Accept", "text/html")],
        corpo: Corpo::Niente,
    })?;
    if risposta.stato == 404 {
        return Err(AppError::new(ErrorCode::SpotifyNotPublic));
    }
    if !risposta.e_andata() {
        return Err(rete.stato_a_errore(&risposta, &url));
    }
    interpreta(&risposta.testo(), riferimento).ok_or_else(|| {
        AppError::new(ErrorCode::SpotifyResolveFailed)
            .with_message("la pagina del riquadro non contiene l'elenco dei brani")
    })
}

/// Interpreta l'HTML della pagina. Funzione pura: si prova senza rete.
#[must_use]
pub fn interpreta(html: &str, riferimento: &Riferimento) -> Option<SpotifyContent> {
    let dati = estrai_json(html)?;
    let entita = trova_entita(&dati, 0);
    let voci = trova_tracklist(&dati, 0).unwrap_or_default();

    let titolo = entita
        .and_then(|e| testo(e, "name"))
        .or_else(|| entita.and_then(|e| testo(e, "title")))
        .unwrap_or_else(|| riferimento.id.clone());
    let copertina = entita.and_then(|e| e.get("coverArt")).and_then(immagine);
    let sottotitolo = entita.and_then(|e| testo(e, "subtitle"));

    // Su un album il nome del contenitore È il nome dell'album, e il suo
    // sottotitolo è l'interprete. Portarli su ogni brano è quel che impedisce a
    // un album letto da qui di spezzarsi in tanti album senza nome — il vecchio
    // albero lo faceva, ed è la correzione che vale la pena portare intatta.
    let (album, album_artist) = match riferimento.genere {
        aether_domain::spotify::SpotifyKind::Album => (Some(titolo.clone()), sottotitolo.clone()),
        _ => (None, None),
    };

    let mut brani = Vec::with_capacity(voci.len());
    for voce in &voci {
        let Some(titolo_brano) = testo(voce, "title").or_else(|| testo(voce, "name")) else {
            continue;
        };
        let interprete = testo(voce, "subtitle").or_else(|| testo(voce, "artist"));
        brani.push(SpotifyTrack {
            title: titolo_brano,
            artist: interprete.clone(),
            album: album.clone(),
            album_artist: album_artist.clone().or(interprete),
            duration_ms: durata(voce),
            cover_url: voce.get("coverArt").and_then(immagine),
            ..SpotifyTrack::default()
        });
    }

    if brani.is_empty() && titolo.is_empty() {
        return None;
    }
    // Un brano singolo non ha una `trackList`: l'entità stessa è il brano.
    if brani.is_empty() && riferimento.genere == aether_domain::spotify::SpotifyKind::Track {
        brani.push(SpotifyTrack {
            title: titolo.clone(),
            artist: sottotitolo.clone(),
            duration_ms: entita.and_then(durata),
            cover_url: copertina.clone(),
            ..SpotifyTrack::default()
        });
    }

    Some(SpotifyContent {
        kind: riferimento.genere,
        id: riferimento.id.clone(),
        title: titolo,
        author: sottotitolo,
        cover_url: copertina,
        declared_total: trova_totale(&dati, 0),
        tracks: brani,
        source: SpotifySource::Embed,
    })
}

// ── estrazione del JSON dalla pagina ────────────────────────────────────────

/// Il JSON della pagina, in una qualunque delle forme in cui Spotify lo mette.
fn estrai_json(html: &str) -> Option<Value> {
    if let Some(v) = da_script(html, "__NEXT_DATA__").and_then(|t| serde_json::from_str(&t).ok()) {
        return Some(v);
    }
    // La forma recente: base64 dentro `<script id="initial-state">`.
    if let Some(codificato) = da_script(html, "initial-state")
        && let Some(byte) = base64_decodifica(codificato.trim())
        && let Ok(testo) = String::from_utf8(byte)
        && let Ok(v) = serde_json::from_str::<Value>(&testo)
    {
        return Some(v);
    }
    None
}

/// Il contenuto del tag `<script>` che porta l'identificativo indicato.
///
/// Si taglia con `find` e non con un lettore di HTML: qui non si sta
/// interpretando un documento, si sta prendendo il testo fra due marcatori noti,
/// e portarsi dietro un analizzatore di HTML per questo sarebbe sproporzionato.
fn da_script(html: &str, identificativo: &str) -> Option<String> {
    let dove = html.find(identificativo)?;
    let resto = html.get(dove..)?;
    let apertura = resto.find('>')?;
    let dopo = resto.get(apertura.checked_add(1)?..)?;
    let chiusura = dopo.find("</script>")?;
    Some(dopo.get(..chiusura)?.trim().to_owned())
}

/// Decodifica base64 standard, tollerando spazi e riempimento mancante.
///
/// Scritta a mano invece di aggiungere una dipendenza: sono trenta righe usate
/// in un punto solo, e il workspace non ha altri motivi per portarsi un
/// codificatore.
fn base64_decodifica(testo: &str) -> Option<Vec<u8>> {
    let valore = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };

    // Solo la capacità iniziale: tre byte ogni quattro caratteri, arrotondati
    // per difetto. Se il conto è corto di qualche byte il `Vec` cresce da sé.
    #[expect(
        clippy::integer_division,
        reason = "è una stima di capacità, non un risultato: il resto non ha significato"
    )]
    let mut fuori = Vec::with_capacity(testo.len() / 4 * 3);
    let mut accumulatore = 0_u32;
    let mut quanti = 0_u32;
    for byte in testo.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let sei = valore(byte)?;
        accumulatore = (accumulatore << 6) | sei;
        quanti = quanti.saturating_add(6);
        if quanti >= 8 {
            quanti = quanti.saturating_sub(8);
            let estratto = (accumulatore >> quanti) & 0xff;
            fuori.push(u8::try_from(estratto).unwrap_or(0));
        }
    }
    Some(fuori)
}

// ── navigazione del JSON ────────────────────────────────────────────────────

/// Il primo array non vuoto sotto una chiave `trackList`.
fn trova_tracklist(nodo: &Value, profondita: usize) -> Option<Vec<Value>> {
    if profondita > PROFONDITA_MASSIMA {
        return None;
    }
    let oggetto = nodo.as_object()?;
    if let Some(elenco) = oggetto.get("trackList").and_then(Value::as_array)
        && !elenco.is_empty()
    {
        return Some(elenco.clone());
    }
    for figlio in oggetto.values() {
        if let Some(trovato) = trova_tracklist(figlio, profondita.saturating_add(1)) {
            return Some(trovato);
        }
    }
    None
}

/// Il nodo che porta il nome e la copertina del contenitore.
fn trova_entita(nodo: &Value, profondita: usize) -> Option<&Value> {
    if profondita > PROFONDITA_MASSIMA {
        return None;
    }
    let oggetto = nodo.as_object()?;
    if oggetto.get("name").is_some_and(Value::is_string)
        && (oggetto.contains_key("coverArt") || oggetto.contains_key("trackList"))
    {
        return Some(nodo);
    }
    oggetto
        .values()
        .find_map(|figlio| trova_entita(figlio, profondita.saturating_add(1)))
}

/// Un totale dichiarato, se la pagina ne porta uno.
///
/// È il numero che permette di accorgersi del troncamento. Quando non c'è, chi
/// mostra il risultato deve comunque avvertire che si è letto da qui.
fn trova_totale(nodo: &Value, profondita: usize) -> Option<u32> {
    if profondita > PROFONDITA_MASSIMA {
        return None;
    }
    let oggetto = nodo.as_object()?;
    for chiave in ["totalCount", "trackCount", "total"] {
        if let Some(n) = oggetto.get(chiave).and_then(Value::as_u64)
            && let Ok(n) = u32::try_from(n)
            && n > 0
        {
            return Some(n);
        }
    }
    oggetto
        .values()
        .find_map(|figlio| trova_totale(figlio, profondita.saturating_add(1)))
}

fn testo(v: &Value, chiave: &str) -> Option<String> {
    v.get(chiave)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// La durata, che nel riquadro è a volte un numero e a volte un oggetto.
fn durata(v: &Value) -> Option<u64> {
    let campo = v.get("duration")?;
    campo
        .as_u64()
        .or_else(|| campo.get("totalMilliseconds").and_then(Value::as_u64))
}

/// L'indirizzo dell'immagine più grande fra le sorgenti.
fn immagine(v: &Value) -> Option<String> {
    let sorgenti = v.get("sources")?.as_array()?;
    sorgenti
        .last()
        .and_then(|s| testo(s, "url"))
        .or_else(|| sorgenti.first().and_then(|s| testo(s, "url")))
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::spotify::SpotifyKind;

    fn rif(genere: SpotifyKind) -> Riferimento {
        Riferimento {
            genere,
            id: "37i9dQZF1DXcBWIGoYBM5M".to_owned(),
        }
    }

    fn pagina_next(json: &str) -> String {
        format!(
            r#"<html><body><script id="__NEXT_DATA__" type="application/json">{json}</script></body></html>"#
        )
    }

    #[test]
    fn una_playlist_si_legge_dal_next_data() {
        let json = r#"{"props":{"pageProps":{"state":{"data":{"entity":{
            "name":"La mia playlist",
            "subtitle":"Tizio",
            "coverArt":{"sources":[{"url":"https://piccola"},{"url":"https://grande"}]},
            "trackList":[
                {"title":"Uno","subtitle":"Artista A","duration":180000},
                {"title":"Due","subtitle":"Artista B","duration":{"totalMilliseconds":200000}}
            ]}}}}}}"#;
        let Some(c) = interpreta(&pagina_next(json), &rif(SpotifyKind::Playlist)) else {
            panic!("la pagina si deve leggere");
        };
        assert_eq!(c.title, "La mia playlist");
        assert_eq!(c.author.as_deref(), Some("Tizio"));
        assert_eq!(c.cover_url.as_deref(), Some("https://grande"));
        assert_eq!(c.tracks.len(), 2);
        assert_eq!(c.tracks.first().map(|b| b.duration_ms), Some(Some(180_000)));
        assert_eq!(
            c.tracks.get(1).map(|b| b.duration_ms),
            Some(Some(200_000)),
            "la durata è a volte un numero e a volte un oggetto"
        );
        assert_eq!(c.source, SpotifySource::Embed);
    }

    #[test]
    fn un_album_porta_il_suo_nome_su_ogni_brano() {
        // Senza questo, un album letto da qui si spezza in tanti album senza
        // nome quanti sono i brani.
        let json = r#"{"d":{"entity":{
            "name":"OK Computer","subtitle":"Radiohead",
            "coverArt":{"sources":[{"url":"https://c"}]},
            "trackList":[{"title":"Airbag","subtitle":"Radiohead","duration":284000}]}}}"#;
        let Some(c) = interpreta(&pagina_next(json), &rif(SpotifyKind::Album)) else {
            panic!("la pagina si deve leggere");
        };
        let Some(b) = c.tracks.first() else {
            panic!("c'è un brano");
        };
        assert_eq!(b.album.as_deref(), Some("OK Computer"));
        assert_eq!(b.album_artist.as_deref(), Some("Radiohead"));
    }

    #[test]
    fn un_brano_singolo_non_ha_una_tracklist() {
        let json = r#"{"d":{"entity":{
            "name":"Karma Police","subtitle":"Radiohead","duration":264066,
            "coverArt":{"sources":[{"url":"https://c"}]}}}}"#;
        let Some(c) = interpreta(&pagina_next(json), &rif(SpotifyKind::Track)) else {
            panic!("la pagina si deve leggere");
        };
        assert_eq!(c.tracks.len(), 1);
        assert_eq!(
            c.tracks.first().map(|b| b.title.as_str()),
            Some("Karma Police")
        );
        assert_eq!(c.tracks.first().and_then(|b| b.duration_ms), Some(264_066));
    }

    #[test]
    fn il_totale_dichiarato_rende_visibile_il_troncamento() {
        // Il caso per cui esiste `declared_total`: il riquadro ne mostra due su
        // trecento, e senza il totale sembrerebbe una playlist di due brani.
        let json = r#"{"d":{"entity":{"name":"Grande","coverArt":{"sources":[]},
            "trackList":[{"title":"Uno"},{"title":"Due"}]},"totalCount":300}}"#;
        let Some(c) = interpreta(&pagina_next(json), &rif(SpotifyKind::Playlist)) else {
            panic!("la pagina si deve leggere");
        };
        assert_eq!(c.truncation(), Some((2, 300)));
    }

    #[test]
    fn si_legge_anche_la_forma_in_base64() {
        let json = r#"{"entity":{"name":"Da base64","coverArt":{"sources":[]},
            "trackList":[{"title":"Uno","duration":1000}]}}"#;
        // base64 dello stesso JSON, calcolato dal nostro codificatore di prova.
        let codificato = base64_codifica(json.as_bytes());
        let html = format!(r#"<script id="initial-state" type="text/plain">{codificato}</script>"#);
        let Some(c) = interpreta(&html, &rif(SpotifyKind::Playlist)) else {
            panic!("la forma in base64 si deve leggere");
        };
        assert_eq!(c.title, "Da base64");
        assert_eq!(c.tracks.len(), 1);
    }

    #[test]
    fn una_pagina_senza_niente_non_inventa_niente() {
        assert!(interpreta("<html></html>", &rif(SpotifyKind::Playlist)).is_none());
        assert!(interpreta(&pagina_next("non è json"), &rif(SpotifyKind::Playlist)).is_none());
    }

    #[test]
    fn il_base64_fa_il_giro() {
        for originale in [
            &b""[..],
            &b"a"[..],
            &b"ab"[..],
            &b"abc"[..],
            &b"abcd"[..],
            "accentate: perché è così".as_bytes(),
        ] {
            let codificato = base64_codifica(originale);
            assert_eq!(
                base64_decodifica(&codificato).as_deref(),
                Some(originale),
                "giro fallito su {originale:?}"
            );
        }
        // Spazi e a capo dentro la stringa non la rompono.
        assert_eq!(
            base64_decodifica("YWJj\n ZGVm").as_deref(),
            Some(&b"abcdef"[..])
        );
        assert_eq!(base64_decodifica("non-base64!"), None);
    }

    /// Codificatore base64, solo per provare il decodificatore.
    fn base64_codifica(dati: &[u8]) -> String {
        const ALFABETO: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut fuori = String::new();
        for pezzo in dati.chunks(3) {
            let a = pezzo.first().copied().unwrap_or(0);
            let b = pezzo.get(1).copied().unwrap_or(0);
            let c = pezzo.get(2).copied().unwrap_or(0);
            let n = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
            let indice = |scarto: u32| usize::try_from((n >> scarto) & 0x3f).unwrap_or(0);
            fuori.push(char::from(ALFABETO[indice(18)]));
            fuori.push(char::from(ALFABETO[indice(12)]));
            fuori.push(if pezzo.len() > 1 {
                char::from(ALFABETO[indice(6)])
            } else {
                '='
            });
            fuori.push(if pezzo.len() > 2 {
                char::from(ALFABETO[indice(0)])
            } else {
                '='
            });
        }
        fuori
    }
}
