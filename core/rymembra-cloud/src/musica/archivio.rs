//! I file del cloud come li vede chi li apre: la scansione, la riproduzione,
//! l'analisi.
//!
//! # Tre lettori, non uno
//!
//! Un brano del cloud si legge in tre modi diversi, e per ragioni diverse:
//!
//! - **la scansione** e la rilettura dei tag passano da [`MusicFiles::open`],
//!   che qui è un [`LettoreTag`]: finestre piccole, un tetto di byte, e
//!   **niente cache**. Ventimila brani letti per i loro tag sono ventimila
//!   teste e code: tenerle su disco riempirebbe in un pomeriggio una cache
//!   di duecento megabyte che esiste per i riascolti;
//! - **la riproduzione**, gli attacchi dei testi e il ponte di «Ascolta
//!   insieme» passano da [`ArchivioNuvola::apri_brano`], che è la sorgente del
//!   servizio dentro una [`ConCache`]: il brano intero, quel che arriva resta;
//! - **l'analisi** passa da [`CopieComplete`], che apre solo una copia già
//!   completa in cache e non va mai in rete: «mentre suonano» vuol dire
//!   misurare quel che l'ascolto ha già portato sul disco, non scaricare
//!   apposta.
//!
//! Lo specchio dice dove sta ogni file e cosa c'è sotto ogni cartella; i
//! depositi sanno parlare coi servizi. Questo tipo li tiene insieme e non sa
//! niente di nessuno dei due servizi in particolare.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use rymembra_app::files::{Camminata, Limiti, MusicFiles, ReadSeek, Voce};
use rymembra_app::nuvola::SpecchioCondiviso;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola::{Servizio, TagNoti, VoceRemota, conto_di as conto_del_percorso};
use rymembra_domain::scan_plan::DiscoveredFile;
use rymembra_net::flusso::Sorgente;

use super::{Cache, Chiave, Deposito, LettoreTag, limiti_di};

/// I depositi collegati, per conto.
///
/// Li scrive il desktop quando un conto si collega o si scollega; li legge
/// chi apre un file. Un conto che non è qui è un conto non collegato su questo
/// PC — anche se le sue cartelle sono nella libreria, arrivate con la
/// sincronia.
pub type Depositi = Arc<RwLock<HashMap<String, Arc<dyn Deposito>>>>;

/// I file del cloud, per chi li apre.
pub struct ArchivioNuvola {
    specchio: SpecchioCondiviso,
    depositi: Depositi,
    cache: Arc<Cache>,
    /// Un brano del cloud sta suonando: la scansione legge con metà delle
    /// richieste insieme, per non contendere la linea a quel che si ascolta.
    suona: AtomicBool,
    /// I conti il cui permesso il servizio ha respinto per sempre: vedi
    /// [`ArchivioNuvola::segna_respinto`].
    respinti: RwLock<HashSet<String>>,
}

impl std::fmt::Debug for ArchivioNuvola {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchivioNuvola")
            .field("suona", &self.suona.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Un file trovato: di quale conto, con quale deposito, e cosa dice lo
/// specchio di lui.
struct Trovato {
    conto: String,
    deposito: Arc<dyn Deposito>,
    voce: VoceRemota,
}

impl ArchivioNuvola {
    /// Mette insieme specchio, depositi e cache.
    #[must_use]
    pub fn nuovo(specchio: SpecchioCondiviso, depositi: Depositi, cache: Arc<Cache>) -> Self {
        Self {
            specchio,
            depositi,
            cache,
            suona: AtomicBool::new(false),
            respinti: RwLock::default(),
        }
    }

    /// Dice che il servizio ha respinto per sempre il permesso di un conto
    /// (`cloud.authExpired`, `cloud.appRejected`: il consenso revocato, l'app
    /// sospesa), o che non lo respinge più.
    ///
    /// Un conto respinto è come un disco di rete spento: i suoi file ci sono,
    /// ma non si possono leggere finché qualcuno non ricollega. Le sue radici
    /// non si raggiungono ([`MusicFiles::radice_raggiungibile`]), e la
    /// scansione le salta senza giudicare nessuno dei suoi brani: prima la
    /// rilettura dei tag li provava uno per uno, e finivano fra gli
    /// «illeggibili» del resoconto. Lo scrive chi vede l'esito di ogni giro
    /// del conto (il desktop), e lo toglie quando un giro riesce, quando il
    /// conto si ricollega o la sua app cambia, e quando si scollega.
    pub fn segna_respinto(&self, conto: &str, respinto: bool) {
        let mut respinti = self
            .respinti
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if respinto {
            respinti.insert(conto.to_owned());
        } else {
            respinti.remove(conto);
        }
    }

    /// Il conto è segnato respinto: vedi [`Self::segna_respinto`].
    #[must_use]
    pub fn respinto(&self, conto: &str) -> bool {
        self.respinti
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(conto)
    }

    /// Dice se adesso sta suonando un brano del cloud.
    pub fn suona_dal_cloud(&self, suona: bool) {
        self.suona.store(suona, Ordering::Relaxed);
    }

    /// La cache dei brani.
    #[must_use]
    pub fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }

    /// Il deposito di un conto, se è collegato.
    #[must_use]
    pub fn deposito(&self, conto: &str) -> Option<Arc<dyn Deposito>> {
        self.depositi
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(conto)
            .cloned()
    }

    /// Il conto e la voce dello specchio per un percorso, senza rete.
    #[must_use]
    pub fn voce_di(&self, percorso: &str) -> Option<(String, VoceRemota)> {
        let specchio = self.specchio.read().unwrap_or_else(PoisonError::into_inner);
        specchio
            .voce(percorso)
            .map(|(conto, voce)| (conto.to_owned(), voce.clone()))
    }

    /// La chiave della copia in cache di un percorso, se il servizio ha
    /// detto una versione.
    #[must_use]
    pub fn chiave(&self, percorso: &str) -> Option<Chiave> {
        let (conto, voce) = self.voce_di(percorso)?;
        Chiave::di(&conto, &voce)
    }

    /// Il file, il suo conto e il suo deposito.
    ///
    /// # Errori
    ///
    /// `cloud.notConnected` se il conto non è collegato su questo PC,
    /// `cloud.notFound` se lo specchio quel file non lo conosce (più).
    fn trova(&self, percorso: &str) -> Result<Trovato, AppError> {
        let conto = conto_del_percorso(percorso).unwrap_or_default().to_owned();
        let Some(deposito) = self.deposito(&conto) else {
            return Err(AppError::new(ErrorCode::CloudNotConnected {
                servizio: nome_servizio(&conto),
            }));
        };
        let Some((conto, voce)) = self.voce_di(percorso) else {
            return Err(AppError::new(ErrorCode::CloudNotFound)
                .with_message(format!("lo specchio non conosce {percorso}")));
        };
        Ok(Trovato {
            conto,
            deposito,
            voce,
        })
    }

    /// Il brano intero, dentro la cache, e la sua lunghezza.
    ///
    /// È la via della riproduzione, degli attacchi dei testi e del ponte di
    /// «Ascolta insieme»: quel che passa di qui resta nella cache (se è
    /// accesa), e un riascolto parte dal disco.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::trova`] e di [`Deposito::sorgente`].
    pub fn apri_brano(&self, percorso: &str) -> Result<(Box<dyn Sorgente>, u64), AppError> {
        let trovato = self.trova(percorso)?;
        let sorgente = trovato.deposito.sorgente(&trovato.voce)?;
        let chiave = Chiave::di(&trovato.conto, &trovato.voce);
        Ok((
            Box::new(self.cache.con(chiave, sorgente)),
            trovato.voce.dimensione,
        ))
    }

    /// La copia completa in cache di un percorso, se c'è.
    #[must_use]
    pub fn copia_completa(&self, percorso: &str) -> Option<PathBuf> {
        self.cache.copia_completa(&self.chiave(percorso)?)
    }

    /// La pagina del file sul sito del servizio; `None` se il servizio non
    /// ne ha una.
    ///
    /// # Errori
    ///
    /// Quelli di chi apre il brano: `cloud.notConnected` per un conto non
    /// collegato, `cloud.notFound` per un percorso che lo specchio non ha.
    pub fn indirizzo_web(&self, percorso: &str) -> Result<Option<String>, AppError> {
        let trovato = self.trova(percorso)?;
        Ok(trovato.deposito.indirizzo_web(&trovato.voce))
    }
}

/// Il nome leggibile del servizio di un conto, per le frasi d'errore.
fn nome_servizio(conto: &str) -> String {
    Servizio::del_conto(conto).map_or_else(String::new, |s| s.nome().to_owned())
}

impl MusicFiles for ArchivioNuvola {
    fn walk(&self, root: &str) -> Result<Camminata, AppError> {
        let mut file = Vec::new();
        let completa = self.walk_a_rate(root, &mut |voce| {
            if let Voce::File(trovato) = voce {
                file.push(trovato);
            }
            ControlFlow::Continue(())
        })?;
        Ok(Camminata { file, completa })
    }

    /// Dallo specchio, senza rete.
    ///
    /// Una radice il cui primo elenco non è ancora arrivato in fondo consegna
    /// quel che c'è **e** sé stessa come ramo perso: la scansione mette le
    /// righe dei file già noti e non toglie niente — che è esattamente la
    /// passata A mentre l'elenco scorre ancora.
    fn walk_a_rate(
        &self,
        root: &str,
        su_voce: &mut dyn FnMut(Voce) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        // Una copia, e il lucchetto si lascia subito: chi consuma le voci può
        // metterci il tempo che vuole, e il filo del conto deve poter scrivere
        // lo specchio intanto.
        let (sotto, completa) = {
            let specchio = self.specchio.read().unwrap_or_else(PoisonError::into_inner);
            (specchio.sotto(root), specchio.e_completa(root))
        };
        let Some(sotto) = sotto else {
            let _ = su_voce(Voce::RamoPerso(root.to_owned()));
            return Ok(false);
        };
        for voce in sotto {
            let consegna = if voce.cartella {
                Voce::Cartella
            } else {
                Voce::File(DiscoveredFile {
                    path: voce.percorso,
                    size_bytes: voce.dimensione,
                    modified_ms: voce.modificato_ms,
                })
            };
            if su_voce(consegna).is_break() {
                return Ok(false);
            }
        }
        if !completa {
            let _ = su_voce(Voce::RamoPerso(root.to_owned()));
            return Ok(false);
        }
        Ok(true)
    }

    /// Il conto è collegato **e** la cartella sta sotto una radice dello
    /// specchio che è al suo posto
    /// ([`rymembra_app::nuvola::Specchio::raggiungibile`]).
    ///
    /// Un conto arrivato con la sincronia e mai collegato su questo PC non è
    /// raggiungibile: la scansione non giudica le sue righe, che restano.
    /// Nemmeno una radice rinominata o tolta sul servizio, per la stessa
    /// ragione. Una sottocartella sì: è quel che i cambiamenti fanno
    /// riguardare, e fino alla 3.2.0 di prova si saltava sempre.
    ///
    /// E nemmeno un conto collegato il cui permesso il servizio ha respinto
    /// per sempre ([`ArchivioNuvola::segna_respinto`]): senza rete in mezzo,
    /// solo quel che il filo del conto ha già visto.
    fn radice_raggiungibile(&self, root: &str) -> bool {
        let Some(conto) = conto_del_percorso(root) else {
            return false;
        };
        if self.deposito(conto).is_none() || self.respinto(conto) {
            return false;
        }
        self.specchio
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .raggiungibile(root)
    }

    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
        let trovato = self.trova(path)?;
        let sorgente = trovato.deposito.sorgente(&trovato.voce)?;
        Ok(Box::new(LettoreTag::nuovo(
            sorgente,
            Some(trovato.voce.dimensione),
        )))
    }

    fn limiti(&self, root: &str) -> Limiti {
        let conto = conto_del_percorso(root).unwrap_or_default();
        let mut limiti = match self.deposito(conto) {
            Some(deposito) => deposito.limiti(),
            None => Servizio::del_conto(conto).map_or(Limiti::DISCO, limiti_di),
        };
        if self.suona.load(Ordering::Relaxed) {
            // Metà, per difetto: uno spostamento e non una divisione.
            limiti.in_volo = (limiti.in_volo >> 1).max(2);
        }
        limiti
    }

    fn tag_noti(&self, path: &str) -> Option<TagNoti> {
        self.voce_di(path).and_then(|(_, voce)| voce.tag)
    }
}

/// I file del cloud per l'analisi: solo le copie già complete in cache.
///
/// L'analisi non scarica niente apposta (la scelta «mentre suonano»): un
/// brano del cloud si misura dopo che un ascolto lo ha portato per intero
/// sul disco, e qui si apre quella copia come un file qualunque. Senza copia,
/// `cloud.notInCache` — che non è un guasto del file, e chi misura non lo
/// segna come illeggibile.
#[derive(Debug)]
pub struct CopieComplete {
    archivio: Arc<ArchivioNuvola>,
}

impl CopieComplete {
    /// Sopra un archivio.
    #[must_use]
    pub const fn di(archivio: Arc<ArchivioNuvola>) -> Self {
        Self { archivio }
    }
}

impl MusicFiles for CopieComplete {
    /// L'analisi non cammina: misura brani dati per identificativo.
    fn walk(&self, root: &str) -> Result<Camminata, AppError> {
        let _ = root;
        Ok(Camminata {
            file: Vec::new(),
            completa: false,
        })
    }

    fn radice_raggiungibile(&self, root: &str) -> bool {
        let _ = root;
        false
    }

    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
        let Some(copia) = self.archivio.copia_completa(path) else {
            return Err(AppError::new(ErrorCode::CloudNotInCache));
        };
        let file = std::fs::File::open(&copia)
            .map_err(|err| rymembra_app::files::io_error(&copia.to_string_lossy(), &err))?;
        Ok(Box::new(std::io::BufReader::with_capacity(64 * 1024, file)))
    }
}

#[cfg(test)]
mod prove {
    use std::collections::HashSet;
    use std::io::Read as _;
    use std::time::Duration;

    use rymembra_app::nuvola::{RadiceSalvata, Specchio};
    use rymembra_domain::nuvola::Cambio;
    use rymembra_net::http::Pezzo;

    use super::*;
    use crate::musica::Identita;

    /// Un servizio in memoria: un file solo, di byte noti.
    #[derive(Debug)]
    struct Finto {
        byte: Vec<u8>,
    }

    #[derive(Debug)]
    struct DaFinto {
        byte: Vec<u8>,
    }

    impl Sorgente for DaFinto {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
            let inizio = usize::try_from(da)
                .unwrap_or(usize::MAX)
                .min(self.byte.len());
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(usize::MAX))
                .min(self.byte.len());
            Ok(Pezzo {
                byte: self.byte.get(inizio..fine).unwrap_or_default().to_vec(),
                totale: Some(self.byte.len() as u64),
                url_finale: None,
            })
        }
    }

    impl Deposito for Finto {
        fn servizio(&self) -> Servizio {
            Servizio::OneDrive
        }
        fn chi(&self) -> Result<Identita, AppError> {
            Ok(Identita {
                id_stabile: "d".into(),
                nome: "prova".into(),
            })
        }
        fn cartelle(&self, dentro: Option<&str>) -> Result<Vec<VoceRemota>, AppError> {
            let _ = dentro;
            Ok(Vec::new())
        }
        fn cursore_adesso(&self) -> Result<String, AppError> {
            Ok("c".into())
        }
        fn elenca(
            &self,
            cima: &str,
            pagina: &mut dyn FnMut(Vec<VoceRemota>) -> ControlFlow<()>,
        ) -> Result<bool, AppError> {
            let _ = (cima, pagina);
            Ok(true)
        }
        fn cambiamenti(
            &self,
            cursore: &str,
            attesa: Duration,
        ) -> Result<(Vec<Cambio>, String), AppError> {
            let _ = attesa;
            Ok((Vec::new(), cursore.to_owned()))
        }
        fn sorgente(&self, voce: &VoceRemota) -> Result<Box<dyn Sorgente>, AppError> {
            let _ = voce;
            Ok(Box::new(DaFinto {
                byte: self.byte.clone(),
            }))
        }
        fn indirizzo_web(&self, voce: &VoceRemota) -> Option<String> {
            Some(format!("https://esempio/{}", voce.id))
        }
    }

    const CONTO: &str = "onedrive-abcdefgh";

    fn voce(id: &str, genitore: Option<&str>, nome: &str, cartella: bool) -> VoceRemota {
        VoceRemota {
            id: id.into(),
            genitore: genitore.map(str::to_owned),
            nome: nome.into(),
            cartella,
            dimensione: if cartella { 0 } else { 6 },
            modificato_ms: 1_000,
            versione: (!cartella).then(|| "v1".to_owned()),
            tag: (!cartella).then(|| TagNoti {
                titolo: Some("Intro".into()),
                ..TagNoti::default()
            }),
        }
    }

    /// Uno specchio con `Musica/Rock/01.flac`, la radice `Musica` scelta.
    fn archivio(
        completa: bool,
        collegato: bool,
    ) -> (Arc<ArchivioNuvola>, String, tempfile::TempDir) {
        let mut specchio = Specchio::default();
        let _ = specchio.metti(
            CONTO,
            vec![
                voce("cima", None, "", true),
                voce("musica", Some("cima"), "Musica", true),
                voce("rock", Some("musica"), "Rock", true),
                voce("f1", Some("rock"), "01.flac", false),
            ],
        );
        let radice = specchio.percorso(CONTO, "musica").unwrap_or_default();
        specchio.aggiungi_radice(RadiceSalvata {
            percorso: radice.clone(),
            conto: CONTO.into(),
            cima: "musica".into(),
            completa: false,
        });
        if completa {
            let elencati: HashSet<String> = ["musica", "rock", "f1"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            let _ = specchio.chiudi_elenco(CONTO, "musica", &elencati);
        }
        let depositi: Depositi = Arc::default();
        if collegato {
            depositi
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(
                    CONTO.into(),
                    Arc::new(Finto {
                        byte: b"ABCDEF".to_vec(),
                    }),
                );
        }
        let cartella = tempfile::tempdir().expect("prova");
        let cache = Cache::apri(cartella.path(), 1024 * 1024, true).expect("prova");
        (
            Arc::new(ArchivioNuvola::nuovo(
                Arc::new(RwLock::new(specchio)),
                depositi,
                cache,
            )),
            radice,
            cartella,
        )
    }

    fn cammina(archivio: &ArchivioNuvola, radice: &str) -> (Vec<String>, usize, bool, bool) {
        let mut file = Vec::new();
        let mut cartelle = 0;
        let mut ramo_perso = false;
        let completa = archivio
            .walk_a_rate(radice, &mut |voce| {
                match voce {
                    Voce::File(f) => file.push(f.path),
                    Voce::Cartella => cartelle += 1,
                    Voce::RamoPerso(_) => ramo_perso = true,
                }
                ControlFlow::Continue(())
            })
            .expect("prova");
        (file, cartelle, completa, ramo_perso)
    }

    #[test]
    fn la_camminata_viene_dallo_specchio() {
        let (archivio, radice, _cartella) = archivio(true, true);
        let (file, cartelle, completa, ramo_perso) = cammina(&archivio, &radice);
        assert_eq!(file, ["nuvola://onedrive-abcdefgh/Musica/Rock/01.flac"]);
        assert!(cartelle >= 1);
        assert!(completa && !ramo_perso);
        assert!(archivio.radice_raggiungibile(&radice));
    }

    #[test]
    fn un_elenco_a_meta_consegna_i_file_ma_non_e_completo() {
        // È la passata A mentre l'elenco scorre: le righe entrano, niente si
        // toglie.
        let (archivio, radice, _cartella) = archivio(false, true);
        let (file, _, completa, ramo_perso) = cammina(&archivio, &radice);
        assert_eq!(file.len(), 1);
        assert!(!completa && ramo_perso);
    }

    #[test]
    fn una_sottocartella_si_raggiunge_una_radice_spostata_no() {
        // I cambiamenti fanno riguardare la cartella del file cambiato: fino
        // alla 3.2.0 di prova una sottocartella non si raggiungeva mai, e il
        // file nuovo aspettava la «Scansiona» dopo.
        let (archivio, radice, _cartella) = archivio(true, true);
        let rock = format!("{radice}/Rock");
        assert!(archivio.radice_raggiungibile(&rock));

        // La cartella scelta cambia nome sul servizio: la sua radice è un
        // disco staccato, non una cartella vuota.
        let _ = archivio
            .specchio
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .applica(
                CONTO,
                vec![Cambio::Voce(voce("musica", Some("cima"), "Musica 2", true))],
            );
        assert!(!archivio.radice_raggiungibile(&radice));
        assert!(!archivio.radice_raggiungibile(&rock));
        let (file, _, completa, ramo_perso) = cammina(&archivio, &radice);
        assert!(file.is_empty() && !completa && ramo_perso);
    }

    #[test]
    fn un_conto_respinto_non_si_raggiunge_come_un_disco_spento() {
        // Un consenso che il servizio ha respinto per sempre (`cloud.authExpired`,
        // `cloud.appRejected`): i file ci sono, ma non si possono leggere finché
        // qualcuno non ricollega. Come un NAS spento, la scansione salta le sue
        // radici e non giudica nessuno dei suoi brani, invece di contarli fra
        // gli «illeggibili».
        let (archivio, radice, _cartella) = archivio(true, true);
        let rock = format!("{radice}/Rock");
        assert!(archivio.radice_raggiungibile(&radice));
        archivio.segna_respinto(CONTO, true);
        assert!(archivio.respinto(CONTO));
        assert!(!archivio.radice_raggiungibile(&radice));
        assert!(!archivio.radice_raggiungibile(&rock));
        // Un altro conto non c'entra.
        assert!(!archivio.respinto("dropbox-zzzzzzzz"));
        // Il giro dopo riesce (o il conto si ricollega): torna raggiungibile.
        archivio.segna_respinto(CONTO, false);
        assert!(!archivio.respinto(CONTO));
        assert!(archivio.radice_raggiungibile(&radice));
        assert!(archivio.radice_raggiungibile(&rock));
    }

    #[test]
    fn senza_conto_collegato_la_radice_non_si_raggiunge_e_non_si_apre() {
        let (archivio, radice, _cartella) = archivio(true, false);
        assert!(!archivio.radice_raggiungibile(&radice));
        let errore = archivio
            .open("nuvola://onedrive-abcdefgh/Musica/Rock/01.flac")
            .err()
            .map(|e| e.code().kind());
        assert_eq!(
            errore,
            Some(rymembra_domain::errors::ErrorCodeKind::CloudNotConnected)
        );
    }

    #[test]
    fn una_radice_sconosciuta_non_e_completa_e_non_toglie_niente() {
        let (archivio, _, _cartella) = archivio(true, true);
        let (file, _, completa, ramo_perso) =
            cammina(&archivio, "nuvola://onedrive-abcdefgh/Altro");
        assert!(file.is_empty() && !completa && ramo_perso);
    }

    #[test]
    fn i_tag_noti_vengono_dallo_specchio() {
        let (archivio, _, _cartella) = archivio(true, true);
        let tag = archivio.tag_noti("nuvola://onedrive-abcdefgh/Musica/Rock/01.flac");
        assert_eq!(tag.and_then(|t| t.titolo).as_deref(), Some("Intro"));
    }

    #[test]
    fn i_tag_si_leggono_senza_cache_e_il_brano_con_la_cache() {
        let (archivio, _, _cartella) = archivio(true, true);
        let percorso = "nuvola://onedrive-abcdefgh/Musica/Rock/01.flac";
        let mut letto = Vec::new();
        archivio
            .open(percorso)
            .and_then(|mut r| {
                r.read_to_end(&mut letto)
                    .map_err(|e| rymembra_app::files::io_error(percorso, &e))
            })
            .expect("prova");
        assert_eq!(letto, b"ABCDEF");
        // Il lettore dei tag non lascia niente nella cache.
        assert!(archivio.copia_completa(percorso).is_none());
        // La riproduzione sì: letta per intero, la copia è completa.
        let (sorgente, lunghezza) = archivio.apri_brano(percorso).expect("prova");
        assert_eq!(lunghezza, 6);
        let pezzo = sorgente.pezzo(0, 6).expect("prova");
        assert_eq!(pezzo.byte, b"ABCDEF");
        assert!(archivio.copia_completa(percorso).is_some());
        // E l'analisi la apre come un file.
        let copie = CopieComplete::di(Arc::clone(&archivio));
        let mut misurato = Vec::new();
        copie
            .open(percorso)
            .and_then(|mut r| {
                r.read_to_end(&mut misurato)
                    .map_err(|e| rymembra_app::files::io_error(percorso, &e))
            })
            .expect("prova");
        assert_eq!(misurato, b"ABCDEF");
    }

    #[test]
    fn l_analisi_non_va_in_rete() {
        let (archivio, _, _cartella) = archivio(true, true);
        let copie = CopieComplete::di(archivio);
        let errore = copie
            .open("nuvola://onedrive-abcdefgh/Musica/Rock/01.flac")
            .err()
            .map(|e| e.code().kind());
        assert_eq!(
            errore,
            Some(rymembra_domain::errors::ErrorCodeKind::CloudNotInCache)
        );
    }

    #[test]
    fn mentre_suona_un_brano_del_cloud_la_scansione_rallenta() {
        let (archivio, radice, _cartella) = archivio(true, true);
        let prima = archivio.limiti(&radice).in_volo;
        archivio.suona_dal_cloud(true);
        let durante = archivio.limiti(&radice).in_volo;
        assert!(durante < prima && durante >= 2, "{prima} → {durante}");
        archivio.suona_dal_cloud(false);
        assert_eq!(archivio.limiti(&radice).in_volo, prima);
    }

    #[test]
    fn l_indirizzo_web_lo_compone_il_deposito() {
        let (scollegato, _, _altra) = archivio(true, false);
        let (archivio, _, _cartella) = archivio(true, true);
        assert_eq!(
            archivio
                .indirizzo_web("nuvola://onedrive-abcdefgh/Musica/Rock/01.flac")
                .expect("collegato")
                .as_deref(),
            Some("https://esempio/f1")
        );
        // Col conto scollegato si dice perché, non «file non trovato».
        let errore = scollegato
            .indirizzo_web("nuvola://onedrive-abcdefgh/Musica/Rock/01.flac")
            .expect_err("scollegato");
        assert_eq!(
            errore.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::CloudNotConnected
        );
    }
}
