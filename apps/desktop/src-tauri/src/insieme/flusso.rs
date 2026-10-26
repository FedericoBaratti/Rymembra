//! [`DaPari`]: i byte di una voce che sta sul disco di chi ospita, letti in
//! rete col suo gettone.
//!
//! È una [`Sorgente`] di `rymembra-net`, la stessa forma dei cataloghi: il
//! lettore la mette dentro `FlussoHttp::da`, che le dà la finestra che scorre,
//! la lettura anticipata e l'annullo, e il motore non sa da dove arrivino i
//! byte. Quel che resta da fare qui è tradurre «dammi questi byte» in letture
//! di [`LettoreRemoto`].
//!
//! # Perché a blocchi
//!
//! `FlussoHttp` chiede fino a due mebibyte per volta, e quando chi ascolta
//! salta altrove smette di volerli: lo dice con `avanti()`, una chiusura che si
//! può solo **interrogare**. Il trasporto invece si interrompe con una
//! bandiera (`AtomicBool`) che guarda mentre aspetta. Le due cose non si
//! parlano senza un filo che stia lì a girare la prima nella seconda, e un filo
//! per lettura è un prezzo che non vale. Allora si legge a blocchi di
//! [`BLOCCO`] e si chiede `avanti()` fra l'uno e l'altro: in una rete di casa
//! un blocco sono una ventina di millisecondi, e un salto aspetta al più
//! quelli.
//!
//! La bandiera del trasporto c'è lo stesso, ed è quella **della sessione**:
//! quando la sessione finisce si alza, e una lettura ferma su un host sparito
//! torna subito invece di aspettare la scadenza della rete.
//!
//! # Niente cause dal trasporto
//!
//! Gli errori di [`LettoreRemoto`] passano con il loro codice e senza la loro
//! causa: il lettore scrive nel diario i guasti d'apertura, e il testo libero
//! di un guasto di rete può contenere un indirizzo.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_insieme::{Gettone, LettoreRemoto};
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::Pezzo;

/// Quanto si chiede al trasporto per volta: vedi il `//!`.
pub const BLOCCO: u64 = 256 * 1024;

/// Chi sa leggere i pezzi di un gettone. È [`LettoreRemoto`]; nelle prove, un
/// file finto in memoria.
pub trait Pezzi: Send + Sync + std::fmt::Debug + 'static {
    /// Fino a `quanti` byte da `da`, e la dimensione del file: vedi
    /// [`LettoreRemoto::pezzo`].
    ///
    /// # Errori
    ///
    /// Quelli di [`LettoreRemoto::pezzo`].
    fn pezzo(
        &self,
        gettone: &Gettone,
        da: u64,
        quanti: u64,
        annullo: Option<&AtomicBool>,
    ) -> Result<(Vec<u8>, u64), AppError>;
}

impl Pezzi for LettoreRemoto {
    fn pezzo(
        &self,
        gettone: &Gettone,
        da: u64,
        quanti: u64,
        annullo: Option<&AtomicBool>,
    ) -> Result<(Vec<u8>, u64), AppError> {
        Self::pezzo(self, gettone, da, quanti, annullo)
    }
}

/// I byte di un gettone, come [`Sorgente`]. Vedi il `//!`.
#[derive(Debug)]
pub struct DaPari<L: Pezzi = LettoreRemoto> {
    lettore: L,
    gettone: Gettone,
    /// Alzata quando la sessione finisce.
    sessione_finita: Arc<AtomicBool>,
}

impl<L: Pezzi> DaPari<L> {
    /// Legge `gettone` con `lettore`, finché la sessione non finisce.
    pub const fn nuovo(lettore: L, gettone: Gettone, sessione_finita: Arc<AtomicBool>) -> Self {
        Self {
            lettore,
            gettone,
            sessione_finita,
        }
    }

    fn leggi(&self, da: u64, quanti: u64, avanti: &dyn Fn() -> bool) -> Result<Pezzo, AppError> {
        // La capacità ha un tetto suo: `quanti` lo decide chi chiede, e un
        // numero sbagliato non deve diventare un'allocazione da gigabyte prima
        // ancora di aver letto un byte.
        let capienza = usize::try_from(quanti.min(2 * 1024 * 1024)).unwrap_or(0);
        let mut byte = Vec::with_capacity(capienza);
        let mut totale = None;
        let mut letti = 0_u64;
        while letti < quanti {
            if !avanti() || self.sessione_finita.load(Ordering::Acquire) {
                return Err(AppError::new(ErrorCode::InternalAborted {
                    what: Some("lettura in rete".to_owned()),
                }));
            }
            let chiesti = (quanti - letti).min(BLOCCO);
            let (blocco, dimensione) = self
                .lettore
                .pezzo(
                    &self.gettone,
                    da.saturating_add(letti),
                    chiesti,
                    Some(&self.sessione_finita),
                )
                .map_err(|err| AppError::new(err.code().clone()))?;
            totale = Some(dimensione);
            let arrivati = u64::try_from(blocco.len()).unwrap_or(u64::MAX);
            byte.extend_from_slice(&blocco);
            letti = letti.saturating_add(arrivati);
            // Meno di quanti chiesti è la fine del file, come per il
            // trasporto: chiederne ancora vorrebbe dire una lettura vuota.
            if arrivati < chiesti {
                break;
            }
        }
        Ok(Pezzo {
            byte,
            totale,
            url_finale: None,
        })
    }
}

impl<L: Pezzi> Sorgente for DaPari<L> {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.leggi(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        self.leggi(da, quanti, avanti)
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::io::{Read as _, Seek as _, SeekFrom};
    use std::sync::Mutex;

    use rymembra_net::flusso::FlussoHttp;

    /// Un file finto, in memoria, che ricorda ogni richiesta.
    #[derive(Debug)]
    struct Finto {
        byte: Vec<u8>,
        richieste: Mutex<Vec<(u64, u64)>>,
        scaduto: AtomicBool,
    }

    impl Finto {
        fn di(n: usize) -> Arc<Self> {
            Arc::new(Self {
                byte: (0..n).map(|i| u8::try_from(i % 251).unwrap()).collect(),
                richieste: Mutex::new(Vec::new()),
                scaduto: AtomicBool::new(false),
            })
        }
    }

    impl Pezzi for Arc<Finto> {
        fn pezzo(
            &self,
            gettone: &Gettone,
            da: u64,
            quanti: u64,
            annullo: Option<&AtomicBool>,
        ) -> Result<(Vec<u8>, u64), AppError> {
            assert_eq!(gettone.0, [7; 16], "il gettone della voce");
            assert!(quanti <= BLOCCO, "mai più di un blocco: {quanti}");
            self.richieste.lock().unwrap().push((da, quanti));
            if annullo.is_some_and(|a| a.load(Ordering::Acquire)) {
                return Err(AppError::new(ErrorCode::InternalAborted { what: None }));
            }
            if self.scaduto.load(Ordering::Acquire) {
                return Err(AppError::new(ErrorCode::InsiemeGettoneScaduto)
                    .with_cause("10.0.0.5:47811 ha chiuso"));
            }
            let totale = self.byte.len();
            let inizio = usize::try_from(da).unwrap().min(totale);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap())
                .min(totale);
            Ok((self.byte[inizio..fine].to_vec(), totale as u64))
        }
    }

    fn da_pari(file: &Arc<Finto>) -> (DaPari<Arc<Finto>>, Arc<AtomicBool>) {
        let finita = Arc::new(AtomicBool::new(false));
        (
            DaPari::nuovo(Arc::clone(file), Gettone([7; 16]), Arc::clone(&finita)),
            finita,
        )
    }

    #[test]
    fn un_intervallo_si_legge_a_blocchi_e_torna_esatto() {
        let file = Finto::di(700_000);
        let (sorgente, _) = da_pari(&file);
        let pezzo = sorgente.pezzo(100, 600_000).unwrap();
        assert_eq!(pezzo.byte, file.byte[100..600_100]);
        assert_eq!(pezzo.totale, Some(700_000));
        assert_eq!(
            *file.richieste.lock().unwrap(),
            vec![
                (100, BLOCCO),
                (100 + BLOCCO, BLOCCO),
                (100 + 2 * BLOCCO, 600_000 - 2 * BLOCCO)
            ]
        );
    }

    #[test]
    fn la_fine_del_file_ferma_le_richieste() {
        let file = Finto::di(300_000);
        let (sorgente, _) = da_pari(&file);
        let pezzo = sorgente.pezzo(200_000, 2 * 1024 * 1024).unwrap();
        assert_eq!(pezzo.byte.len(), 100_000);
        assert_eq!(
            file.richieste.lock().unwrap().len(),
            1,
            "una lettura corta basta"
        );
        let vuoto = sorgente.pezzo(400_000, 10).unwrap();
        assert!(vuoto.byte.is_empty());
    }

    #[test]
    fn chi_non_vuole_piu_i_byte_ferma_la_lettura_fra_un_blocco_e_l_altro() {
        let file = Finto::di(2_000_000);
        let (sorgente, _) = da_pari(&file);
        let chiamate = std::cell::Cell::new(0);
        let avanti = || {
            chiamate.set(chiamate.get() + 1);
            chiamate.get() <= 2
        };
        let err = sorgente
            .pezzo_finche(0, 2 * 1024 * 1024, &avanti)
            .unwrap_err();
        assert_eq!(err.code().kind().code(), "internal.aborted");
        assert_eq!(file.richieste.lock().unwrap().len(), 2);
    }

    #[test]
    fn la_sessione_finita_ferma_anche_la_lettura_in_corso() {
        let file = Finto::di(10);
        let (sorgente, finita) = da_pari(&file);
        finita.store(true, Ordering::Release);
        let err = sorgente.pezzo(0, 10).unwrap_err();
        assert_eq!(err.code().kind().code(), "internal.aborted");
        assert!(file.richieste.lock().unwrap().is_empty());
    }

    #[test]
    fn un_guasto_del_trasporto_passa_col_codice_e_senza_la_causa() {
        let file = Finto::di(10);
        file.scaduto.store(true, Ordering::Release);
        let (sorgente, _) = da_pari(&file);
        let err = sorgente.pezzo(0, 10).unwrap_err();
        assert_eq!(err.code().kind().code(), "insieme.gettoneScaduto");
        assert_eq!(err.cause(), None, "niente indirizzi nel diario");
    }

    #[test]
    fn dentro_un_flusso_si_legge_e_si_salta_come_un_file() {
        let file = Finto::di(5_000_000);
        let (sorgente, _) = da_pari(&file);
        let mut flusso = FlussoHttp::da(Box::new(sorgente)).unwrap();
        assert_eq!(flusso.lunghezza(), Some(5_000_000));
        let mut testa = [0_u8; 1000];
        flusso.read_exact(&mut testa).unwrap();
        assert_eq!(testa[..], file.byte[..1000]);
        flusso.seek(SeekFrom::Start(4_000_000)).unwrap();
        let mut dentro = [0_u8; 4096];
        flusso.read_exact(&mut dentro).unwrap();
        assert_eq!(dentro[..], file.byte[4_000_000..4_004_096]);
        flusso.seek(SeekFrom::End(-10)).unwrap();
        let mut coda = Vec::new();
        flusso.read_to_end(&mut coda).unwrap();
        assert_eq!(coda, file.byte[4_999_990..]);
    }
}

/// La misura chiesta dalla specifica: quanto costa riaprire una voce in rete
/// e posizionarla con esattezza, su 127.0.0.1, con un MP3 vero. Due reti
/// vere, nessuna uscita audio: il decodificatore gira a vuoto.
///
/// `cargo test -p rymembra-desktop misura_riapri -- --ignored --nocapture`
#[cfg(test)]
#[expect(
    clippy::integer_division,
    reason = "secondi interi dai millisecondi, per una riga di misura"
)]
mod misura {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use rymembra_insieme::{Evento, FileServito, Identita, Impostazioni, Rete};
    use rymembra_net::flusso::FlussoHttp;
    use rymembra_play::decodifica::Decodificatore;

    /// Il flusso in rete come lo vede il motore: `remoto() = false`, come
    /// l'involucro del lettore, così il salto è quello accurato.
    struct Involucro(FlussoHttp);

    impl std::io::Read for Involucro {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl std::io::Seek for Involucro {
        fn seek(&mut self, verso: std::io::SeekFrom) -> std::io::Result<u64> {
            self.0.seek(verso)
        }
    }

    impl rymembra_play::Flusso for Involucro {
        fn lunghezza(&self) -> Option<u64> {
            self.0.lunghezza()
        }
    }

    fn rete(
        nome: &str,
    ) -> (
        Rete,
        std::sync::mpsc::Receiver<Evento>,
        rymembra_insieme::ChiavePubblica,
    ) {
        let identita = Identita::nuova().unwrap();
        let chiave = identita.pubblica();
        let (rete, eventi) = Rete::avvia(Impostazioni {
            identita,
            nome: nome.to_owned(),
            lega: IpAddr::V4(Ipv4Addr::LOCALHOST),
            porta: 47_900,
            adesso_ns: rymembra_play::tempo::adesso_ns,
        })
        .unwrap();
        (rete, eventi, chiave)
    }

    /// Il lettore vero, che conta richieste e byte.
    #[derive(Debug, Clone)]
    struct Contato {
        lettore: rymembra_insieme::LettoreRemoto,
        conti: Arc<std::sync::Mutex<(u64, u64)>>,
    }

    impl Pezzi for Contato {
        fn pezzo(
            &self,
            gettone: &Gettone,
            da: u64,
            quanti: u64,
            annullo: Option<&AtomicBool>,
        ) -> Result<(Vec<u8>, u64), AppError> {
            let esito = self.lettore.pezzo(gettone, da, quanti, annullo);
            if let Ok((byte, _)) = &esito {
                let mut conti = self.conti.lock().unwrap();
                conti.0 += 1;
                conti.1 += byte.len() as u64;
            }
            esito
        }
    }

    fn apri_e_cerca(
        lettore: &rymembra_insieme::LettoreRemoto,
        gettone: Gettone,
        ms: u64,
    ) -> (Duration, Duration) {
        let inizio = Instant::now();
        let conti = Arc::new(std::sync::Mutex::new((0, 0)));
        let contato = Contato {
            lettore: lettore.clone(),
            conti: Arc::clone(&conti),
        };
        let sorgente = DaPari::nuovo(contato, gettone, Arc::new(AtomicBool::new(false)));
        let flusso = FlussoHttp::da(Box::new(sorgente)).unwrap();
        let mut dec = Decodificatore::apri(
            rymembra_play::Sorgente {
                track_id: -1,
                media: Box::new(Involucro(flusso)),
                estensione: Some("mp3".to_owned()),
                durata_ms: 0,
                replaygain_db: None,
                picco: None,
                segmento: None,
            },
            48_000,
            2,
        )
        .unwrap();
        let aperto = inizio.elapsed();
        dec.cerca(ms).unwrap();
        let mut campioni = Vec::new();
        dec.prossimo(&mut campioni).unwrap();
        let tutto = inizio.elapsed();
        let (richieste, byte) = *conti.lock().unwrap();
        eprintln!(
            "[misura]   a {} s: {richieste} letture, {} KiB, {} ms",
            ms / 1_000,
            byte / 1024,
            tutto.as_millis()
        );
        (aperto, tutto)
    }

    #[test]
    #[ignore = "misura: due reti vere su 127.0.0.1 e un MP3 del sito"]
    fn misura_riapri_di_una_voce_in_rete() {
        let mp3 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../site/audio/machina.mp3");
        let meta = std::fs::metadata(&mp3).unwrap();
        let modificato_ms = i64::try_from(
            meta.modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let (host, _eventi_host, chiave_host) = rete("host");
        let (membro, eventi_membro, chiave_membro) = rete("membro");
        host.pari(vec![chiave_membro], vec![]);
        membro.pari(vec![chiave_host], vec![]);
        let porta = host.ospita().unwrap();
        // Chi ospita lascia leggere solo i membri della sessione.
        host.ammessi(vec![chiave_membro]);
        let indirizzo = SocketAddr::from((Ipv4Addr::LOCALHOST, porta));
        let id = membro.collega(indirizzo);
        let fine = Instant::now() + Duration::from_secs(10);
        loop {
            let resto = fine.saturating_duration_since(Instant::now());
            match eventi_membro.recv_timeout(resto).unwrap() {
                Evento::Aperta { id: aperta, .. } if aperta == id => break,
                Evento::Chiusa { codice, .. } => panic!("chiusa: {codice:?}"),
                _ => {}
            }
        }
        let gettone = Gettone::nuovo().unwrap();
        host.servi(
            gettone,
            FileServito {
                percorso: mp3.clone(),
                dimensione: meta.len(),
                modificato_ms,
            },
        );
        let lettore = membro.lettore(indirizzo, chiave_host);
        // Il primo giro scalda le connessioni di lettura; poi tre punti.
        let _ = apri_e_cerca(&lettore, gettone, 1_000);
        // La banda del trasporto da sola: il file intero a blocchi.
        let inizio = Instant::now();
        let mut da = 0;
        while da < meta.len() {
            let (byte, _) = lettore.pezzo(&gettone, da, BLOCCO, None).unwrap();
            da += byte.len() as u64;
        }
        let ms = inizio.elapsed().as_millis().max(1);
        eprintln!(
            "[misura] il file intero ({} KiB) a blocchi da {} KiB: {ms} ms, {} KiB/s",
            meta.len() / 1024,
            BLOCCO / 1024,
            u128::from(meta.len() / 1024) * 1000 / ms
        );
        for ms in [10_000_u64, 90_000, 170_000] {
            let mut tempi = Vec::new();
            for _ in 0..5 {
                tempi.push(apri_e_cerca(&lettore, gettone, ms));
            }
            tempi.sort_by_key(|t| t.1);
            let (aperto, tutto) = tempi[2];
            eprintln!(
                "[misura] riapri a {} s: apertura {} ms, apertura+salto+primo blocco {} ms (mediana di 5; min {} ms, max {} ms)",
                ms / 1_000,
                aperto.as_millis(),
                tutto.as_millis(),
                tempi[0].1.as_millis(),
                tempi[4].1.as_millis(),
            );
        }
        // Lo stesso dal disco, per confronto.
        for ms in [10_000_u64, 170_000] {
            let inizio = Instant::now();
            let mut dec = Decodificatore::apri(
                rymembra_play::Sorgente {
                    track_id: -1,
                    media: Box::new(std::fs::File::open(&mp3).unwrap()),
                    estensione: Some("mp3".to_owned()),
                    durata_ms: 0,
                    replaygain_db: None,
                    picco: None,
                    segmento: None,
                },
                48_000,
                2,
            )
            .unwrap();
            dec.cerca(ms).unwrap();
            let mut campioni = Vec::new();
            dec.prossimo(&mut campioni).unwrap();
            eprintln!(
                "[misura] dal disco a {} s: {} ms",
                ms / 1_000,
                inizio.elapsed().as_millis()
            );
        }
        membro.ferma();
        host.ferma();
    }
}

/// Fase 2 su tre capi veri, su 127.0.0.1: chi ospita e due membri. Il primo
/// membro serve un file suo e apre la connessione di servizio; chi ospita
/// inoltra il gettone; il secondo membro lo legge attraverso chi ospita, con
/// un `DaPari` dentro un `FlussoHttp`, come farebbe il suo lettore. Nessun
/// audio: si confrontano i byte.
#[cfg(test)]
mod tre_capi {
    use super::*;
    use std::io::{Read as _, Seek as _, SeekFrom};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use rymembra_insieme::{ChiavePubblica, Evento, FileServito, Identita, Impostazioni, Rete};
    use rymembra_net::flusso::FlussoHttp;

    fn rete(nome: &str) -> (Rete, Receiver<Evento>, ChiavePubblica) {
        let identita = Identita::nuova().unwrap();
        let chiave = identita.pubblica();
        let (rete, eventi) = Rete::avvia(Impostazioni {
            identita,
            nome: nome.to_owned(),
            lega: IpAddr::V4(Ipv4Addr::LOCALHOST),
            porta: 47_960,
            adesso_ns: rymembra_play::tempo::adesso_ns,
        })
        .unwrap();
        (rete, eventi, chiave)
    }

    fn aspetta_aperta(eventi: &Receiver<Evento>) {
        let fine = Instant::now() + Duration::from_secs(10);
        loop {
            let resto = fine.saturating_duration_since(Instant::now());
            match eventi.recv_timeout(resto).expect("la connessione si apre") {
                Evento::Aperta { .. } => return,
                Evento::Chiusa { codice, .. } => panic!("chiusa: {codice:?}"),
                _ => {}
            }
        }
    }

    #[test]
    fn un_membro_legge_il_brano_di_un_altro_membro_attraverso_chi_ospita() {
        let cartella = tempfile::tempdir().unwrap();
        let percorso = cartella.path().join("brano.flac");
        let byte: Vec<u8> = (0..1_500_000_u32).map(|i| (i % 253) as u8).collect();
        std::fs::write(&percorso, &byte).unwrap();

        let (host, _eventi_host, chiave_host) = rete("host");
        let (marco, eventi_marco, chiave_marco) = rete("marco");
        let (luca, eventi_luca, chiave_luca) = rete("luca");
        host.pari(vec![chiave_marco, chiave_luca], vec![]);
        marco.pari(vec![chiave_host], vec![]);
        luca.pari(vec![chiave_host], vec![]);
        let porta = host.ospita().unwrap();
        // I membri della sessione: solo loro leggono i gettoni di chi ospita.
        host.ammessi(vec![chiave_marco, chiave_luca]);
        let indirizzo = SocketAddr::from((Ipv4Addr::LOCALHOST, porta));
        marco.collega(indirizzo);
        luca.collega(indirizzo);
        aspetta_aperta(&eventi_marco);
        aspetta_aperta(&eventi_luca);

        // Marco serve il suo file e apre la connessione di servizio; chi
        // ospita inoltra il gettone a lui.
        let gettone = Gettone::nuovo().unwrap();
        marco.servi(gettone, FileServito::da_percorso(&percorso).unwrap());
        marco.offri(indirizzo, chiave_host);
        assert!(host.inoltra(gettone, chiave_marco));
        // Lo stesso gettone a un altro membro: no, è già di Marco.
        assert!(!host.inoltra(gettone, chiave_luca));

        // Luca legge: la connessione di servizio può metterci un attimo.
        let lettore = luca.lettore(indirizzo, chiave_host);
        let fine = Instant::now() + Duration::from_secs(10);
        loop {
            match lettore.pezzo(&gettone, 0, 16, None) {
                Ok(_) => break,
                Err(err) if Instant::now() < fine => {
                    let _ = err;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(err) => panic!("il gettone di Marco non si legge da Luca: {err:?}"),
            }
        }
        let finita = Arc::new(AtomicBool::new(false));
        let sorgente = DaPari::nuovo(lettore, gettone, Arc::clone(&finita));
        let mut flusso = FlussoHttp::da(Box::new(sorgente)).unwrap();
        assert_eq!(flusso.lunghezza(), Some(1_500_000));
        let mut letti = Vec::new();
        flusso.read_to_end(&mut letti).unwrap();
        assert_eq!(letti, byte, "gli stessi byte, dall'inizio alla fine");
        flusso.seek(SeekFrom::Start(1_000_000)).unwrap();
        let mut dentro = [0_u8; 4096];
        flusso.read_exact(&mut dentro).unwrap();
        assert_eq!(dentro[..], byte[1_000_000..1_004_096]);

        // E chi ospita lo legge senza rete, per suonarlo lui.
        let locale = DaPari::nuovo(host.lettore_locale(), gettone, finita);
        let pezzo = locale.pezzo(500_000, 1_000).unwrap();
        assert_eq!(pezzo.byte, byte[500_000..501_000]);

        // Tolto il gettone, la lettura finisce col perché.
        host.revoca(&gettone);
        let err = luca
            .lettore(indirizzo, chiave_host)
            .pezzo(&gettone, 0, 16, None)
            .unwrap_err();
        assert_eq!(err.code().kind().code(), "insieme.gettoneScaduto");

        luca.ferma();
        marco.ferma();
        host.ferma();
    }
}
