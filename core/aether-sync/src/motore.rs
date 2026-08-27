//! Una passata di sincronia, dall'inizio alla fine.
//!
//! # Cosa fa una passata
//!
//! Elenca il magazzino, legge i documenti degli altri che sono cambiati, li fonde
//! col proprio, e riscrive il proprio se ha qualcosa di nuovo da dire. Su una
//! libreria ferma è **una chiamata sola** e nient'altro: le impronte dicono che
//! non è cambiato niente, e non si scarica né si carica una riga.
//!
//! # Perché il motore va tenuto vivo fra una passata e l'altra
//!
//! Perché è lui a ricordarsi cosa ha già letto. Saltare un documento immutato
//! serve a qualcosa solo se il suo contenuto è ancora in mano: fondere richiede
//! **tutti** i documenti, non solo quelli cambiati, e un motore che si ricostruisse
//! ogni volta dovrebbe riscaricarli tutti per poter saltare quelli che non
//! servono. È la stessa ragione per cui `aether_meta::Fornitori` si costruisce una
//! volta e si tiene: dentro c'è uno stato che vale più dell'oggetto.
//!
//! # Perché un documento illeggibile non ferma la passata
//!
//! Perché fermarsi vorrebbe dire che un file rovinato su un dispositivo blocca la
//! sincronia di tutti gli altri, e che l'unico modo di ripartire è cancellare a
//! mano qualcosa in una cartella. Il documento si salta, il guasto si riferisce, e
//! gli altri quattro dispositivi continuano a parlarsi.

use std::collections::{BTreeMap, BTreeSet};

use aether_domain::errors::AppError;

use crate::documento::{self, Contenuto, Documento};
use crate::fusione::{self, Fuso};
use crate::magazzino::Magazzino;

/// Il tipo che si dichiara scrivendo un documento.
const TIPO: &str = "application/gzip";

/// Un documento che non si è potuto leggere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guasto {
    /// Il file.
    pub nome: String,
    /// Cosa è andato storto, per chi legge i registri.
    pub perche: String,
}

/// A che punto è una passata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Avanzamento {
    /// Quanti documenti sono stati guardati.
    pub fatti: usize,
    /// Quanti in tutto.
    pub totale: usize,
}

/// Com'è andata una passata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Passata {
    /// Quanti documenti sono stati letti davvero.
    pub letti: usize,
    /// Quanti erano già in mano, immutati.
    pub saltati: usize,
    /// Il proprio documento è stato riscritto.
    pub scritto: bool,
    /// L'impronta del proprio contenuto.
    pub impronta: String,
    /// I documenti che non si sono potuti leggere.
    pub guasti: Vec<Guasto>,
    /// I dispositivi visti ma non fusi, perché non ci si fida ancora.
    ///
    /// Sempre vuoto quando la fiducia non è stata configurata.
    pub ignoti: Vec<String>,
}

/// Quel che una passata restituisce: lo stato fuso e com'è andata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Esito {
    /// Lo stato di tutti i dispositivi messi insieme.
    pub fuso: Fuso,
    /// Il resoconto della passata.
    pub passata: Passata,
}

/// Quel che una passata si porta dietro dalla precedente.
///
/// Sta fuori dal motore, e non dentro, per una ragione pratica: il magazzino
/// può cambiare fra una passata e l'altra — un client di Drive si ricostruisce
/// ogni volta che il token si rinfresca — mentre quel che si è già letto no. Un
/// motore che possedesse anche la memoria costringerebbe a buttarla via a ogni
/// rinfresco, cioè a riscaricare tutti i documenti quattro volte all'ora per
/// niente.
#[derive(Debug, Clone, Default)]
pub struct Memoria {
    /// I documenti degli altri, con l'impronta con cui sono arrivati.
    ///
    /// L'impronta è `None` per i magazzini che non la tengono — una cartella non
    /// la tiene — e in quel caso il documento si rilegge sempre. Leggere un file
    /// locale costa meno di ricordarsi se è cambiato.
    conosciuti: BTreeMap<String, (Option<String>, Documento)>,
    /// L'impronta del proprio contenuto, com'è stato scritto l'ultima volta.
    mia_impronta: Option<String>,
}

impl Memoria {
    /// Una memoria vuota: la prima passata leggerà tutto.
    #[must_use]
    pub fn nuova() -> Self {
        Self::default()
    }

    /// Quanti documenti altrui sono in mano.
    #[must_use]
    pub fn quanti(&self) -> usize {
        self.conosciuti.len()
    }

    /// Dimentica tutto.
    ///
    /// Da chiamare quando si cambia magazzino: i documenti di una cartella non
    /// dicono niente su cosa c'è su un Drive, e tenerli vorrebbe dire fondere
    /// insieme due depositi che l'utente ha voluto separati.
    pub fn dimentica(&mut self) {
        *self = Self::default();
    }
}

/// Il motore della sincronia.
pub struct Motore<'a> {
    magazzino: &'a dyn Magazzino,
    dispositivo: String,
    memoria: &'a mut Memoria,
    /// Di chi ci si fida. `None` vuol dire «di tutti».
    ///
    /// La differenza fra `None` e un insieme vuoto è voluta e non è una
    /// sottigliezza: `None` è il comportamento di chi non ha ancora una nozione
    /// di fiducia — l'esempio a riga di comando, le prove — mentre un insieme
    /// vuoto vuol dire «non mi fido ancora di nessuno», che è la condizione
    /// normale di un dispositivo appena aggiunto a una cartella condivisa.
    fidati: Option<BTreeSet<String>>,
}

impl<'a> Motore<'a> {
    /// Un motore su questo magazzino, per questo dispositivo.
    #[must_use]
    pub fn nuovo(
        magazzino: &'a dyn Magazzino,
        dispositivo: impl Into<String>,
        memoria: &'a mut Memoria,
    ) -> Self {
        Self {
            magazzino,
            dispositivo: dispositivo.into(),
            memoria,
            fidati: None,
        }
    }

    /// Fonde soltanto i documenti di questi dispositivi.
    ///
    /// # Perché la fiducia si esercita qui e non altrove
    ///
    /// Perché è l'unico punto in cui i documenti sono ancora separati. Dopo la
    /// fusione non c'è più modo di dire da chi veniva un voto, e togliere
    /// *dopo* quel che un dispositivo ha portato è un'operazione che non
    /// esiste: un `Interruttore` fuso non ricorda chi l'ha acceso.
    ///
    /// I documenti degli altri si leggono comunque, e i loro dispositivi
    /// finiscono in [`Passata::ignoti`]. Serve a poterli mostrare: un elenco di
    /// dispositivi da accettare che si popolasse solo dopo averli accettati
    /// sarebbe una porta la cui maniglia sta dall'altra parte.
    #[must_use]
    pub fn fidandosi_di(mut self, elenco: impl IntoIterator<Item = String>) -> Self {
        self.fidati = Some(elenco.into_iter().collect());
        self
    }

    /// Di chi è questo motore.
    #[must_use]
    pub fn dispositivo(&self) -> &str {
        &self.dispositivo
    }

    /// Una passata.
    ///
    /// `mio` è quel che questo dispositivo ha da dire, già costruito da chi ha il
    /// database. Torna lo stato fuso di tutti, che chi ha il database applica.
    ///
    /// # Errori
    ///
    /// Quelli del magazzino, e solo per i guasti che riguardano **tutta** la
    /// passata: non poter elencare, non poter scrivere il proprio documento. Un
    /// documento altrui illeggibile finisce in [`Passata::guasti`] e la passata
    /// prosegue.
    pub fn passata(
        &mut self,
        mio: &Contenuto,
        adesso_ms: i64,
        avanzando: &dyn Fn(Avanzamento),
    ) -> Result<Esito, AppError> {
        let mio_nome = documento::nome_file(&self.dispositivo);
        let elenco = self.magazzino.elenca()?;

        let documenti: Vec<_> = elenco
            .iter()
            .filter(|file| documento::dispositivo_da_nome(&file.nome).is_some())
            .collect();
        let totale = documenti.len();

        let mut passata = Passata::default();
        let mut visti = Vec::new();

        for (indice, file) in documenti.iter().enumerate() {
            avanzando(Avanzamento {
                fatti: indice,
                totale,
            });
            visti.push(file.nome.clone());

            if file.nome == mio_nome {
                // Il proprio documento non si rilegge per fondersi con sé stesso;
                // si legge una volta sola, all'avvio, per sapere con che impronta
                // era rimasto e non riscriverlo per niente.
                if self.memoria.mia_impronta.is_none() {
                    self.memoria.mia_impronta = self.impronta_remota(&file.id);
                }
                continue;
            }

            let gia = self.memoria.conosciuti.get(&file.nome);
            let immutato = match (
                gia.and_then(|(impronta, _)| impronta.as_ref()),
                &file.impronta,
            ) {
                (Some(mia), Some(sua)) => mia == sua,
                _ => false,
            };
            if immutato {
                passata.saltati += 1;
                continue;
            }

            match self.magazzino.leggi(&file.id) {
                Ok(byte) => match documento::interpreta(&byte) {
                    Ok(letto) => {
                        passata.letti += 1;
                        self.memoria
                            .conosciuti
                            .insert(file.nome.clone(), (file.impronta.clone(), letto));
                    }
                    Err(err) => passata.guasti.push(Guasto {
                        nome: file.nome.clone(),
                        perche: err.to_string(),
                    }),
                },
                Err(err) => passata.guasti.push(Guasto {
                    nome: file.nome.clone(),
                    perche: err.to_string(),
                }),
            }
        }
        avanzando(Avanzamento {
            fatti: totale,
            totale,
        });

        // Un dispositivo il cui file è sparito dal magazzino sparisce anche da
        // qui: qualcuno ha svuotato la cartella, e continuare a contare i suoi
        // ascolti vorrebbe dire tenere in vita un dispositivo che non c'è più.
        self.memoria
            .conosciuti
            .retain(|nome, _| visti.contains(nome));

        let mut tutti: Vec<Documento> = Vec::with_capacity(self.memoria.conosciuti.len() + 1);
        for (_, documento) in self.memoria.conosciuti.values() {
            let fidato = self
                .fidati
                .as_ref()
                .is_none_or(|elenco| elenco.contains(&documento.device));
            if fidato {
                tutti.push(documento.clone());
            } else if !documento.device.is_empty() {
                passata.ignoti.push(documento.device.clone());
            }
        }
        passata.ignoti.sort_unstable();
        passata.ignoti.dedup();
        tutti.push(Documento::nuovo(&self.dispositivo, mio.clone(), adesso_ms));
        let fuso = fusione::fondi(&tutti);

        passata.impronta = documento::impronta(mio)?;
        if self.memoria.mia_impronta.as_deref() != Some(passata.impronta.as_str()) {
            let esistente = elenco
                .iter()
                .find(|file| file.nome == mio_nome)
                .map(|file| file.id.clone());
            let byte = documento::serializza(&Documento::nuovo(
                &self.dispositivo,
                mio.clone(),
                adesso_ms,
            ))?;
            self.magazzino.scrivi(
                &mio_nome,
                esistente.as_deref(),
                TIPO,
                &byte,
                &passata.impronta,
            )?;
            self.memoria.mia_impronta = Some(passata.impronta.clone());
            passata.scritto = true;
        }

        Ok(Esito { fuso, passata })
    }

    /// L'impronta del proprio documento com'è lassù, quando si riesce a leggerlo.
    ///
    /// Un guasto qui non è un guasto della passata: al peggio si riscrive un
    /// documento identico a quello che c'era, che costa una scrittura e non perde
    /// niente.
    fn impronta_remota(&self, id: &str) -> Option<String> {
        let byte = self.magazzino.leggi(id).ok()?;
        let letto = documento::interpreta(&byte).ok()?;
        documento::impronta(&letto.content).ok()
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::magazzino::Cartella;
    use crate::registro::Voto;

    fn niente(_: Avanzamento) {}

    fn con_voto(brano: &str, voto: Voto) -> Contenuto {
        let mut contenuto = Contenuto::default();
        contenuto.voti.insert(brano.to_owned(), voto);
        contenuto
    }

    #[test]
    fn la_prima_passata_scrive_il_proprio_documento() {
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut memoria_motore = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_motore);

        let esito = motore
            .passata(&con_voto("a", Voto { v: 4, at: 10 }), 1_000, &niente)
            .expect("passata");
        assert!(esito.passata.scritto);
        assert_eq!(magazzino.elenca().expect("elenca").len(), 1);
    }

    #[test]
    fn una_passata_a_vuoto_non_riscrive_niente() {
        // La proprietà per cui l'impronta esiste: su una libreria ferma non si
        // tocca il magazzino, e chi lo guarda — Syncthing, il client di Drive — non
        // ha niente da spedire.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut memoria_motore = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_motore);
        let mio = con_voto("a", Voto { v: 4, at: 10 });

        motore.passata(&mio, 1_000, &niente).expect("prima");
        let seconda = motore.passata(&mio, 2_000, &niente).expect("seconda");
        assert!(!seconda.passata.scritto, "riscritto senza motivo");
    }

    #[test]
    fn due_dispositivi_sulla_stessa_cartella_si_vedono() {
        let dove = tempfile::tempdir().expect("cartella");
        let radice = dove.path().join("dispositivi");
        let magazzino = Cartella::nuova(&radice);

        let mut memoria_portatile = Memoria::nuova();
        let mut portatile = Motore::nuovo(&magazzino, "portatile", &mut memoria_portatile);
        let mut memoria_telefono = Memoria::nuova();
        let mut telefono = Motore::nuovo(&magazzino, "telefono", &mut memoria_telefono);

        let suo_voto = con_voto("so-what", Voto { v: 5, at: 100 });
        telefono
            .passata(&suo_voto, 1_000, &niente)
            .expect("telefono");

        let esito = portatile
            .passata(
                &con_voto("blue-in-green", Voto { v: 3, at: 200 }),
                2_000,
                &niente,
            )
            .expect("portatile");

        assert_eq!(esito.passata.letti, 1);
        assert_eq!(esito.fuso.voti.get("so-what").map(|v| v.v), Some(5));
        assert_eq!(esito.fuso.voti.get("blue-in-green").map(|v| v.v), Some(3));
        assert_eq!(esito.fuso.dispositivi.len(), 2);
    }

    #[test]
    fn un_dispositivo_di_cui_non_ci_si_fida_si_vede_ma_non_entra() {
        // La proprietà su cui si regge la fiducia al primo incontro: il suo
        // documento si legge — altrimenti non lo si potrebbe nemmeno mostrare da
        // accettare — ma quel che dice non tocca niente.
        let dove = tempfile::tempdir().expect("cartella");
        let radice = dove.path().join("dispositivi");
        let magazzino = Cartella::nuova(&radice);

        let mut memoria_ignoto = Memoria::nuova();
        let mut ignoto = Motore::nuovo(&magazzino, "ignoto", &mut memoria_ignoto);
        ignoto
            .passata(&con_voto("so-what", Voto { v: 5, at: 100 }), 1_000, &niente)
            .expect("ignoto");

        let mut memoria_mia = Memoria::nuova();
        let mut mio =
            Motore::nuovo(&magazzino, "io", &mut memoria_mia).fidandosi_di(Vec::<String>::new());
        let esito = mio
            .passata(&Contenuto::default(), 2_000, &niente)
            .expect("mia");

        assert_eq!(esito.passata.letti, 1, "il documento si legge comunque");
        assert_eq!(esito.passata.ignoti, vec!["ignoto".to_owned()]);
        assert!(esito.fuso.voti.is_empty(), "ma il suo voto non entra");

        // E dal momento in cui lo si accetta, entra.
        let mut memoria_dopo = Memoria::nuova();
        let mut dopo =
            Motore::nuovo(&magazzino, "io", &mut memoria_dopo).fidandosi_di(["ignoto".to_owned()]);
        let esito = dopo
            .passata(&Contenuto::default(), 3_000, &niente)
            .expect("dopo");
        assert!(esito.passata.ignoti.is_empty());
        assert_eq!(esito.fuso.voti.get("so-what").map(|v| v.v), Some(5));
    }

    #[test]
    fn gli_ascolti_di_due_dispositivi_si_sommano_dopo_una_passata() {
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));

        let mut mio = Contenuto::default();
        mio.ascolti.insert("so-what".to_owned(), 5);
        let mut suo = Contenuto::default();
        suo.ascolti.insert("so-what".to_owned(), 3);

        let mut memoria_telefono = Memoria::nuova();
        let mut telefono = Motore::nuovo(&magazzino, "telefono", &mut memoria_telefono);
        telefono.passata(&suo, 1_000, &niente).expect("telefono");

        let mut memoria_portatile = Memoria::nuova();
        let mut portatile = Motore::nuovo(&magazzino, "portatile", &mut memoria_portatile);
        let esito = portatile.passata(&mio, 2_000, &niente).expect("portatile");
        assert_eq!(
            esito.fuso.ascolti_di("so-what"),
            8,
            "cinque sul portatile e tre sul telefono fanno otto, non cinque"
        );
    }

    #[test]
    fn un_documento_illeggibile_non_ferma_la_passata() {
        let dove = tempfile::tempdir().expect("cartella");
        let radice = dove.path().join("dispositivi");
        let magazzino = Cartella::nuova(&radice);

        let mut memoria_buono = Memoria::nuova();
        let mut buono = Motore::nuovo(&magazzino, "buono", &mut memoria_buono);
        buono
            .passata(&con_voto("a", Voto { v: 4, at: 10 }), 1_000, &niente)
            .expect("buono");
        // Un documento rovinato, col nome giusto e i byte sbagliati.
        magazzino
            .scrivi(
                &documento::nome_file("rotto"),
                None,
                TIPO,
                b"non sono un gzip",
                "x",
            )
            .expect("scrive il rotto");

        let mut memoria_terzo = Memoria::nuova();
        let mut terzo = Motore::nuovo(&magazzino, "terzo", &mut memoria_terzo);
        let esito = terzo
            .passata(&Contenuto::default(), 2_000, &niente)
            .expect("la passata non deve fermarsi");
        assert_eq!(esito.passata.guasti.len(), 1);
        assert_eq!(
            esito.fuso.voti.get("a").map(|v| v.v),
            Some(4),
            "gli altri dispositivi devono continuare a parlarsi"
        );
    }

    #[test]
    fn un_dispositivo_sparito_dalla_cartella_smette_di_contare() {
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));

        let mut andato = Contenuto::default();
        andato.ascolti.insert("a".to_owned(), 7);
        let mut memoria_suo = Memoria::nuova();
        let mut suo = Motore::nuovo(&magazzino, "andato", &mut memoria_suo);
        suo.passata(&andato, 1_000, &niente).expect("scrive");

        let mut memoria_resto = Memoria::nuova();
        let mut resto = Motore::nuovo(&magazzino, "resto", &mut memoria_resto);
        let prima = resto
            .passata(&Contenuto::default(), 2_000, &niente)
            .expect("prima");
        assert_eq!(prima.fuso.ascolti_di("a"), 7);

        magazzino
            .cancella(&documento::nome_file("andato"))
            .expect("cancella");
        let dopo = resto
            .passata(&Contenuto::default(), 3_000, &niente)
            .expect("dopo");
        assert_eq!(dopo.fuso.ascolti_di("a"), 0);
        assert_eq!(dopo.fuso.dispositivi, vec!["resto".to_owned()]);
    }

    #[test]
    fn un_motore_appena_acceso_non_riscrive_un_documento_identico() {
        // È il caso di ogni avvio dell'applicazione: il documento è già lassù, e
        // riscriverlo tale e quale sveglierebbe il client di sincronizzazione per
        // niente.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mio = con_voto("a", Voto { v: 4, at: 10 });

        let mut memoria_prima = Memoria::nuova();
        let mut prima = Motore::nuovo(&magazzino, "uno", &mut memoria_prima);
        prima.passata(&mio, 1_000, &niente).expect("prima");

        let mut memoria_riacceso = Memoria::nuova();
        let mut riacceso = Motore::nuovo(&magazzino, "uno", &mut memoria_riacceso);
        let esito = riacceso.passata(&mio, 5_000, &niente).expect("riacceso");
        assert!(
            !esito.passata.scritto,
            "un motore appena acceso deve riconoscere il proprio documento"
        );
    }

    #[test]
    fn l_avanzamento_arriva_fino_in_fondo() {
        use std::cell::RefCell;
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut memoria_uno = Memoria::nuova();
        let mut uno = Motore::nuovo(&magazzino, "uno", &mut memoria_uno);
        uno.passata(&Contenuto::default(), 1_000, &niente)
            .expect("uno");

        let passi = RefCell::new(Vec::new());
        let mut memoria_due = Memoria::nuova();
        let mut due = Motore::nuovo(&magazzino, "due", &mut memoria_due);
        due.passata(&Contenuto::default(), 2_000, &|avanzamento| {
            passi.borrow_mut().push(avanzamento);
        })
        .expect("due");

        let passi = passi.borrow();
        assert_eq!(
            passi.last().map(|a| (a.fatti, a.totale)),
            Some((1, 1)),
            "l'ultimo avanzamento deve dire che è finita"
        );
    }
}
