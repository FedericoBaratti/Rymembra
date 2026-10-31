//! Un servitore HTTP finto su questa macchina, per le prove di OAuth e dei
//! servizi.
//!
//! Le prove non escono da questo computer, e la cosa da provare è chi riceve
//! che cosa: quante richieste, a quale percorso, con quali intestazioni e quale
//! modulo. Ogni richiesta ricevuta si conserva. Chi risponde è una chiusura con
//! lo stato suo — un contatore per «la seconda volta rispondi altro» — e non un
//! `static`: le prove girano in parallelo, e un contatore condiviso fra due
//! prove è una prova che passa o no a seconda di chi arriva prima.

use std::io::{Read as _, Write as _};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::http::Rete;

/// Una richiesta ricevuta.
#[derive(Debug, Clone)]
pub(crate) struct Vista {
    /// `GET`, `POST`…
    pub metodo: String,
    /// Il percorso con la query, com'è arrivato.
    pub percorso: String,
    /// Le intestazioni, coi nomi in minuscolo.
    pub intestazioni: Vec<(String, String)>,
    /// Il corpo, come testo.
    pub corpo: String,
}

impl Vista {
    /// Il valore di un'intestazione, se c'è.
    pub fn intestazione(&self, nome: &str) -> Option<&str> {
        self.intestazioni
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(nome))
            .map(|(_, v)| v.as_str())
    }

    /// Un parametro della query dell'indirizzo, decodificato.
    pub fn parametro(&self, nome: &str) -> Option<String> {
        let (_, query) = self.percorso.split_once('?')?;
        query.split('&').find_map(|coppia| {
            let (n, v) = coppia.split_once('=')?;
            (n == nome).then(|| decodifica(v))
        })
    }

    /// Un campo del modulo nel corpo, decodificato.
    pub fn campo(&self, nome: &str) -> Option<String> {
        self.corpo.split('&').find_map(|coppia| {
            let (n, v) = coppia.split_once('=')?;
            (n == nome).then(|| decodifica(v))
        })
    }
}

/// Decodifica `%XX` e `+` di un modulo.
fn decodifica(testo: &str) -> String {
    let mut byte = Vec::new();
    let mut resto = testo.bytes();
    while let Some(b) = resto.next() {
        match b {
            b'+' => byte.push(b' '),
            b'%' => {
                let alto = resto.next().and_then(|c| char::from(c).to_digit(16));
                let basso = resto.next().and_then(|c| char::from(c).to_digit(16));
                if let (Some(a), Some(b)) = (alto, basso) {
                    byte.push(u8::try_from(a * 16 + b).unwrap_or(b'?'));
                }
            }
            altro => byte.push(altro),
        }
    }
    String::from_utf8_lossy(&byte).into_owned()
}

/// Chi risponde: riceve la richiesta e l'indirizzo di base del servitore (per
/// scrivere dei `nextLink` che tornino qui), e dà la risposta intera.
type Risponde = dyn Fn(&Vista, &str) -> String + Send + Sync;

/// Il servitore.
pub(crate) struct Finto {
    /// `http://127.0.0.1:<porta>`.
    pub base: String,
    viste: Arc<Mutex<Vec<Vista>>>,
}

impl Finto {
    /// Apre il servitore. Una connessione per richiesta.
    pub fn avvia(risponde: impl Fn(&Vista, &str) -> String + Send + Sync + 'static) -> Self {
        let ascolto =
            std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
        let porta = ascolto.local_addr().expect("la porta scelta").port();
        let base = format!("http://127.0.0.1:{porta}");
        let viste = Arc::new(Mutex::new(Vec::new()));
        let registro = Arc::clone(&viste);
        let risponde: Arc<Risponde> = Arc::new(risponde);
        let base_filo = base.clone();
        std::thread::spawn(move || {
            for connessione in ascolto.incoming() {
                let Ok(mut connessione) = connessione else {
                    continue;
                };
                let _ = connessione.set_read_timeout(Some(Duration::from_secs(5)));
                let Some(vista) = leggi(&mut connessione) else {
                    continue;
                };
                registro.lock().unwrap().push(vista.clone());
                let risposta = risponde(&vista, &base_filo);
                let _ = connessione.write_all(risposta.as_bytes());
                let _ = connessione.flush();
            }
        });
        Self { base, viste }
    }

    /// Le richieste ricevute, nell'ordine.
    pub fn viste(&self) -> Vec<Vista> {
        self.viste.lock().unwrap().clone()
    }

    /// Le richieste ricevute a un percorso che comincia così.
    pub fn viste_a(&self, inizio: &str) -> Vec<Vista> {
        self.viste()
            .into_iter()
            .filter(|v| v.percorso.starts_with(inizio))
            .collect()
    }

    /// Un client verso questo servitore: in chiaro, ma solo verso casa.
    pub fn rete(&self) -> Rete {
        Rete::verso("prova", Duration::from_secs(5), &self.base).expect("casa")
    }
}

/// Legge una richiesta intera: testa e corpo.
fn leggi(connessione: &mut std::net::TcpStream) -> Option<Vista> {
    let mut testa = Vec::new();
    let mut byte = [0_u8; 1];
    while !testa.ends_with(b"\r\n\r\n") {
        match connessione.read(&mut byte) {
            Ok(1) => testa.push(byte[0]),
            _ => return None,
        }
    }
    let testa = String::from_utf8_lossy(&testa).into_owned();
    let mut righe = testa.split("\r\n");
    let prima = righe.next()?;
    let mut pezzi = prima.split(' ');
    let metodo = pezzi.next()?.to_owned();
    let percorso = pezzi.next()?.to_owned();
    let intestazioni: Vec<(String, String)> = righe
        .filter_map(|riga| riga.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let lunghezza = intestazioni
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut corpo = vec![0_u8; lunghezza];
    connessione.read_exact(&mut corpo).ok()?;
    Some(Vista {
        metodo,
        percorso,
        intestazioni,
        corpo: String::from_utf8_lossy(&corpo).into_owned(),
    })
}

/// Una risposta con un corpo.
pub(crate) fn risposta(stato: u16, tipo: &str, corpo: &[u8]) -> String {
    let mut testa = format!(
        "HTTP/1.1 {stato} {}\r\nContent-Type: {tipo}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        motivo(stato),
        corpo.len()
    );
    testa.push_str(&String::from_utf8_lossy(corpo));
    testa
}

/// Una risposta JSON.
pub(crate) fn json(stato: u16, valore: &serde_json::Value) -> String {
    risposta(stato, "application/json", valore.to_string().as_bytes())
}

/// Un rimando.
pub(crate) fn rimando(verso: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {verso}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

/// Un pezzo di un file finto lungo `totale`, coi byte di [`byte_finto`].
///
/// Legge l'intervallo chiesto dall'intestazione `Range`.
pub(crate) fn pezzo_di(vista: &Vista, totale: u64) -> String {
    let (da, fino) = vista
        .intestazione("range")
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
        .and_then(|(a, b)| Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?)))
        .unwrap_or((0, totale.saturating_sub(1)));
    let fino = fino.min(totale.saturating_sub(1));
    let corpo: Vec<u8> = (da..=fino).map(byte_finto).collect();
    format!(
        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {da}-{fino}/{totale}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        corpo.len(),
        String::from_utf8_lossy(&corpo)
    )
}

/// Il byte `n` di un file finto: ASCII, così la risposta resta testo.
pub(crate) fn byte_finto(n: u64) -> u8 {
    b'a' + u8::try_from(n % 26).unwrap_or(0)
}

/// La parola dopo lo stato, per gli stati delle prove.
fn motivo(stato: u16) -> &'static str {
    match stato {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        410 => "Gone",
        429 => "Too Many Requests",
        _ => "Altro",
    }
}
