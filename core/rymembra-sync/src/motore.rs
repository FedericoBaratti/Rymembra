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
//! servono. È la stessa ragione per cui `rymembra_meta::Fornitori` si costruisce una
//! volta e si tiene: dentro c'è uno stato che vale più dell'oggetto.
//!
//! # Il documento di prima è di chi lo ha scritto
//!
//! I documenti col prefisso di prima si leggono come tutti gli altri, perché i
//! dispositivi che non si sono ancora aggiornati continuano a scriverli così.
//! Fa eccezione uno solo: quello **di questo** dispositivo. Rimasto col nome di
//! prima non è un pari con cui fondersi, e la passata lo porta al nome di
//! adesso (o lo toglie, se c'è già il nuovo) invece di contarlo come un altro
//! computer. Vedi `sistema_il_proprio_di_prima`.
//!
//! # Perché un documento illeggibile non ferma la passata
//!
//! Perché fermarsi vorrebbe dire che un file rovinato su un dispositivo blocca la
//! sincronia di tutti gli altri, e che l'unico modo di ripartire è cancellare a
//! mano qualcosa in una cartella. Il documento si salta, il guasto si riferisce, e
//! gli altri quattro dispositivi continuano a parlarsi.

use std::collections::{BTreeMap, BTreeSet};

use rymembra_domain::errors::AppError;

use crate::documento::{self, Contenuto, Documento};
use crate::dominio;
use crate::fusione::{self, Fuso};
use crate::magazzino::{FileRemoto, Magazzino};

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
    /// I documenti che non si sono potuti leggere, con l'impronta con cui
    /// erano lassù e il perché.
    ///
    /// Senza, un documento guasto e immutato si riscaricava e si riapriva a
    /// ogni passata — fino a 64 MiB da decomprimere, ogni volta. Vale solo per
    /// i magazzini che tengono le impronte: una cartella si rilegge comunque.
    /// Il guasto si riferisce lo stesso a ogni passata, finché il file resta.
    scartati: BTreeMap<String, (String, String)>,
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
    /// Il documento pubblicato è `mio` più il segno di capacità
    /// ([`documento::CAPACITA_VOCI_ASSENTI`]): lo aggiunge la passata, così
    /// nessun chiamante può dimenticarlo, e `passata.impronta` è quella del
    /// documento com'è lassù.
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
        let mut elenco = self.magazzino.elenca()?;
        // Il proprio documento può avere ancora il nome di prima. Se lo si è
        // sistemato l'elenco è cambiato, e si riparte da quello vero: non si
        // indovina l'identificativo del file dopo una rinomina, che per una
        // cartella è il nome stesso.
        if self.sistema_il_proprio_di_prima(&elenco, &mio_nome) {
            elenco = self.magazzino.elenca()?;
        }

        let documenti: Vec<_> = elenco
            .iter()
            .filter(|file| documento::dispositivo_da_nome(&file.nome).is_some())
            .collect();
        let totale = documenti.len();

        // Il proprio documento: quello col nome di adesso, o, se la rinomina non
        // è riuscita, quello che porta ancora il nome di prima. Mai tutti e due:
        // se ci sono entrambi vale il nuovo, che è l'unico che questo
        // dispositivo scrive.
        let proprio = elenco
            .iter()
            .find(|file| file.nome == mio_nome)
            .or_else(|| {
                elenco.iter().find(|file| {
                    documento::ha_il_nome_di_prima(&file.nome)
                        && documento::dispositivo_da_nome(&file.nome)
                            == Some(self.dispositivo.as_str())
                })
            });

        let mut passata = Passata::default();
        let mut visti = Vec::new();

        for (indice, file) in documenti.iter().enumerate() {
            avanzando(Avanzamento {
                fatti: indice,
                totale,
            });
            visti.push(file.nome.clone());

            // Il documento di questo dispositivo, con qualunque nome: non è mai un
            // pari. Un proprio documento col nome di prima che resta lì (la
            // rinomina non è riuscita, o c'è anche il nuovo) fondersi con sé
            // stesso vorrebbe dire contare due volte i propri ascolti.
            if documento::dispositivo_da_nome(&file.nome) == Some(self.dispositivo.as_str()) {
                // Non si rilegge per fondersi con sé stesso; si legge una volta
                // sola, all'avvio, per sapere con che impronta era rimasto e non
                // riscriverlo per niente.
                if proprio.is_some_and(|mio| mio.id == file.id)
                    && self.memoria.mia_impronta.is_none()
                {
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
            if let (Some((impronta, perche)), Some(sua)) =
                (self.memoria.scartati.get(&file.nome), &file.impronta)
                && impronta == sua
            {
                passata.guasti.push(Guasto {
                    nome: file.nome.clone(),
                    perche: perche.clone(),
                });
                continue;
            }

            match self.magazzino.leggi(&file.id) {
                Ok(byte) => match documento::interpreta(&byte) {
                    Ok(letto) => {
                        passata.letti += 1;
                        self.memoria.scartati.remove(&file.nome);
                        self.memoria
                            .conosciuti
                            .insert(file.nome.clone(), (file.impronta.clone(), letto));
                    }
                    Err(err) => {
                        // Solo un documento che non si **interpreta** si
                        // ricorda: gli stessi byte daranno lo stesso guasto.
                        // Una lettura fallita (la rete, un permesso) no: va
                        // ritentata alla passata dopo.
                        let perche = err.to_string();
                        if let Some(impronta) = &file.impronta {
                            self.memoria
                                .scartati
                                .insert(file.nome.clone(), (impronta.clone(), perche.clone()));
                        }
                        passata.guasti.push(Guasto {
                            nome: file.nome.clone(),
                            perche,
                        });
                    }
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
        self.memoria.scartati.retain(|nome, _| visti.contains(nome));

        let mut tutti: Vec<Documento> = Vec::with_capacity(self.memoria.conosciuti.len() + 1);
        for (_, documento) in self.memoria.conosciuti.values() {
            let fidato = self
                .fidati
                .as_ref()
                .is_none_or(|elenco| elenco.contains(&documento.device));
            if fidato {
                // Dentro il dominio, a ogni passata e sulla copia: la memoria
                // tiene il documento com'è arrivato, così quel che oggi è
                // rinviato perché sta nel futuro entra da sé quando è ora,
                // senza aspettare che l'altro riscriva il file. Il proprio
                // documento no: è la libreria di qui, e il database ne ha già
                // controllato i valori. Vedi `dominio`.
                let mut sano = documento.clone();
                sano.content =
                    dominio::nel_dominio(sano.content, adesso_ms, dominio::Futuro::Rinvia);
                tutti.push(sano);
            } else if !documento.device.is_empty() {
                passata.ignoti.push(documento.device.clone());
            }
        }
        passata.ignoti.sort_unstable();
        passata.ignoti.dedup();
        // Il documento che si pubblica porta il segno di capacità, e l'impronta
        // si calcola **su questo**: è quella che `scrivi` lascia accanto al file
        // e che `impronta_remota` ritrova rileggendolo. Calcolata su `mio`, che
        // il segno non ce l'ha, non coinciderebbe mai con quella del file letto,
        // e un dispositivo fermo lo riscriverebbe a ogni avvio. Vedi
        // [`documento::CAPACITA_VOCI_ASSENTI`].
        let mut mio_firmato = mio.clone();
        mio_firmato
            .capacita
            .extend(documento::capacita_di_questa_versione());
        let mio_documento = Documento::nuovo(&self.dispositivo, mio_firmato, adesso_ms);
        tutti.push(mio_documento.clone());
        let fuso = fusione::fondi(&tutti);

        passata.impronta = documento::impronta(&mio_documento.content)?;
        // Si scrive se il contenuto è cambiato, **o se il proprio documento
        // lassù non c'è più**: qualcuno ha pulito la cartella, o è finito nel
        // cestino di Drive. L'impronta ricordata dice com'era l'ultima volta che
        // lo si è scritto, non che sia ancora lì; guardando solo quella, il
        // documento tornava al primo ascolto, e fino ad allora chi entrava nella
        // cartella non vedeva niente di questo dispositivo. Senza un file
        // proprio anche `esistente` è `None`: si crea, e un doppione non nasce.
        if proprio.is_none()
            || self.memoria.mia_impronta.as_deref() != Some(passata.impronta.as_str())
        {
            // Se c'è solo il file col nome di prima, si scrive su di lui: dove il
            // magazzino ammette due file con lo stesso nome (Drive) lo rinomina
            // aggiornandolo, senza lasciare un doppione.
            let esistente = proprio.map(|file| file.id.clone());
            let byte = documento::serializza(&mio_documento)?;
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

    /// Porta il proprio documento dal nome di prima a quello di adesso.
    ///
    /// Chi ha cambiato nome al programma si ritrova il proprio documento sotto il
    /// prefisso di allora. Non è un pari da leggere — sarebbe lui stesso, e i suoi
    /// ascolti verrebbero contati due volte — e non va nemmeno lasciato lì: la
    /// cartella avrebbe per sempre due file dello stesso dispositivo.
    ///
    /// - il nuovo manca: si **rinomina** il vecchio, che tiene il suo contenuto
    ///   (e su Drive il suo identificativo);
    /// - ci sono tutti e due: il vecchio si cancella. Quello che conta è il nuovo,
    ///   che questo dispositivo riscrive da sé a ogni cambiamento, e il vecchio
    ///   non ha niente che il nuovo non possa avere.
    ///
    /// Tutto a **miglior sforzo**: un guasto qui non è un guasto della passata,
    /// perché il documento vecchio resta lì e la passata dopo ci riprova.
    /// Restituisce `true` se l'elenco è cambiato e va riletto.
    fn sistema_il_proprio_di_prima(&self, elenco: &[FileRemoto], mio_nome: &str) -> bool {
        let mut nuovo_presente = elenco.iter().any(|file| file.nome == mio_nome);
        let mut cambiato = false;
        for file in elenco {
            let e_mio_di_prima = documento::ha_il_nome_di_prima(&file.nome)
                && documento::dispositivo_da_nome(&file.nome) == Some(self.dispositivo.as_str());
            if !e_mio_di_prima {
                continue;
            }
            if nuovo_presente {
                cambiato |= self.magazzino.cancella(&file.id).is_ok();
            } else if self.magazzino.rinomina(&file.id, mio_nome).is_ok() {
                nuovo_presente = true;
                cambiato = true;
            }
        }
        cambiato
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

    /// Un documento com'è scritto dalla 3.2.0: la stessa busta, e un `content`
    /// che non ha nessun campo `capacita`. Scritto a mano e non con
    /// [`documento::serializza`], che oggi il segno lo conosce.
    fn documento_della_3_2_0(dispositivo: &str, brano: &str, voto: u8, quando: i64) -> Vec<u8> {
        use std::io::Write as _;
        let crudo = format!(
            r#"{{"version":1,"device":"{dispositivo}","generatedAt":500,"content":{{"voti":{{"{brano}":{{"v":{voto},"at":{quando}}}}},"lapidi":{{"brani":{{}},"playlist":{{}}}}}}}}"#
        );
        let mut compressore =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        compressore.write_all(crudo.as_bytes()).expect("comprime");
        compressore.finish().expect("chiude")
    }

    /// Il documento di `dispositivo` com'è nella cartella, già letto.
    fn documento_di(magazzino: &Cartella, dispositivo: &str) -> Documento {
        let nome = documento::nome_file(dispositivo);
        let byte = magazzino.leggi(&nome).expect("il documento c'è");
        documento::interpreta(&byte).expect("si legge")
    }

    #[test]
    fn il_documento_che_si_pubblica_porta_il_segno_di_capacita() {
        // V2: il segno sta dentro `content` (l'impronta si calcola lì) e la 3.2.0
        // lo ignora perché legge per puntatore. Senza, nessuna 3.3.x potrà mai
        // distinguere un pari 3.3.0 da un PC rimasto sulla 3.2.0.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut memoria_motore = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_motore);
        motore
            .passata(&con_voto("a", Voto { v: 4, at: 10 }), 1_000, &niente)
            .expect("passata");

        let letto = documento_di(&magazzino, "uno");
        assert!(
            letto
                .content
                .capacita
                .contains(documento::CAPACITA_VOCI_ASSENTI),
            "il documento non porta il segno: {:?}",
            letto.content.capacita
        );
        // E nel posto giusto: dentro `content`, non nella busta.
        let crudo = {
            use std::io::Read as _;
            let byte = magazzino
                .leggi(&documento::nome_file("uno"))
                .expect("il documento c'è");
            let mut testo = String::new();
            flate2::read::GzDecoder::new(byte.as_slice())
                .read_to_string(&mut testo)
                .expect("si apre");
            serde_json::from_str::<serde_json::Value>(&testo).expect("json")
        };
        assert!(crudo.get("capacita").is_none(), "non va nella busta");
        assert_eq!(
            crudo.pointer("/content/capacita"),
            Some(&serde_json::json!([documento::CAPACITA_VOCI_ASSENTI]))
        );
        assert_eq!(crudo.get("version"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn un_dispositivo_fermo_non_ripubblica_il_proprio_documento_dopo_un_riavvio() {
        // La trappola dell'impronta: `impronta_remota` rilegge il proprio
        // documento e ne calcola l'impronta sul `content` letto. Se il segno non
        // tornasse indietro da `interpreta`, o l'impronta si calcolasse sul
        // `Contenuto` senza segno, i due non coinciderebbero mai e un PC
        // aggiornato e fermo riscriverebbe il suo documento a ogni avvio, per
        // sempre.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mio = con_voto("a", Voto { v: 4, at: 10 });
        {
            let mut memoria_motore = Memoria::nuova();
            let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_motore);
            let prima = motore.passata(&mio, 1_000, &niente).expect("prima");
            assert!(prima.passata.scritto);
            let seconda = motore.passata(&mio, 2_000, &niente).expect("seconda");
            assert!(!seconda.passata.scritto, "riscritto senza motivo");
        }
        // Il riavvio: una memoria nuova, che l'impronta la deve rileggere da lassù.
        let mut memoria_nuova = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_nuova);
        let dopo = motore.passata(&mio, 3_000, &niente).expect("dopo");
        assert!(
            !dopo.passata.scritto,
            "il documento è identico: dopo un riavvio non si riscrive"
        );
    }

    #[test]
    fn il_proprio_documento_della_3_2_0_si_riscrive_una_volta_col_segno() {
        // Un PC che si aggiorna dalla 3.2.0 trova lassù il suo documento di prima,
        // senza segno, con lo stesso contenuto di adesso. Va riscritto — una
        // volta — perché porti il segno; poi basta.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        magazzino
            .scrivi(
                &documento::nome_file("uno"),
                None,
                TIPO,
                &documento_della_3_2_0("uno", "a", 4, 10),
                "impronta-di-prima",
            )
            .expect("il documento di prima");
        let mio = con_voto("a", Voto { v: 4, at: 10 });

        let mut memoria_motore = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_motore);
        let prima = motore.passata(&mio, 1_000, &niente).expect("prima");
        assert!(
            prima.passata.scritto,
            "il documento senza segno va riscritto col segno"
        );
        assert!(
            documento_di(&magazzino, "uno")
                .content
                .capacita
                .contains(documento::CAPACITA_VOCI_ASSENTI)
        );
        let seconda = motore.passata(&mio, 2_000, &niente).expect("seconda");
        assert!(!seconda.passata.scritto, "una volta sola");

        // Anche dopo un riavvio.
        let mut memoria_nuova = Memoria::nuova();
        let mut riavviato = Motore::nuovo(&magazzino, "uno", &mut memoria_nuova);
        let dopo = riavviato.passata(&mio, 3_000, &niente).expect("dopo");
        assert!(!dopo.passata.scritto, "né dopo un riavvio");
    }

    #[test]
    fn il_documento_di_un_pari_della_3_2_0_si_legge_senza_segno_e_senza_perdere_niente() {
        // Il documento di un pari rimasto alla 3.2.0 entra come prima: nessuna
        // regola nuova lo tocca, e il suo voto arriva. «Senza segno» è proprio
        // quel che lo distingue, per chi lo vorrà sapere.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        magazzino
            .scrivi(
                &documento::nome_file("vecchio"),
                None,
                TIPO,
                &documento_della_3_2_0("vecchio", "so-what", 5, 100),
                "x",
            )
            .expect("il documento del pari");

        let mut memoria_motore = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "nuovo", &mut memoria_motore);
        let esito = motore
            .passata(&Contenuto::default(), 2_000, &niente)
            .expect("passata");
        assert_eq!(esito.fuso.voti.get("so-what").map(|v| v.v), Some(5));
        assert!(
            documento_di(&magazzino, "vecchio")
                .content
                .capacita
                .is_empty(),
            "il pari della 3.2.0 non porta il segno"
        );
        assert!(
            documento_di(&magazzino, "nuovo")
                .content
                .capacita
                .contains(documento::CAPACITA_VOCI_ASSENTI)
        );
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
    fn un_proprio_documento_sparito_si_riscrive() {
        // Qualcuno ha pulito la cartella, o il documento è finito nel cestino
        // di Drive. Il contenuto non è cambiato e l'impronta ricordata è quella
        // di sempre: guardando solo quella, il documento non tornava finché
        // qui non si toccava qualcosa, e chi entrava nella cartella non vedeva
        // voti, playlist e ascolti di questo dispositivo.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut memoria_motore = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "uno", &mut memoria_motore);
        let mio = con_voto("a", Voto { v: 4, at: 10 });

        motore.passata(&mio, 1_000, &niente).expect("prima");
        magazzino
            .cancella(&documento::nome_file("uno"))
            .expect("cancella");
        let seconda = motore.passata(&mio, 2_000, &niente).expect("seconda");
        assert!(
            seconda.passata.scritto,
            "il proprio documento manca: va riscritto anche se il contenuto è lo stesso"
        );
        assert_eq!(
            nomi(&magazzino),
            vec![documento::nome_file("uno")],
            "di nuovo nella cartella, e una volta sola"
        );
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

    /// Il nome che un dispositivo ancora «di prima» darebbe al suo documento.
    fn nome_di_prima(dispositivo: &str) -> String {
        format!(
            "{}{dispositivo}.v1.json.gz",
            rymembra_domain::eredita::PREFISSO_DISPOSITIVO
        )
    }

    /// Scrive il documento di un dispositivo sotto il nome di prima, com'è
    /// rimasto sul disco di chi non si è ancora aggiornato.
    fn scrivi_di_prima(magazzino: &Cartella, dispositivo: &str, contenuto: Contenuto) {
        let byte = documento::serializza(&Documento::nuovo(dispositivo, contenuto, 500))
            .expect("serializza");
        magazzino
            .scrivi(&nome_di_prima(dispositivo), None, TIPO, &byte, "x")
            .expect("scrive");
    }

    /// Il contenuto com'è nel documento pubblicato da questa versione: col
    /// segno di capacità che la passata aggiunge.
    ///
    /// Per le prove che vogliono un documento «già a posto»: uno senza il segno
    /// si riscrive una volta (vedi
    /// `il_proprio_documento_della_3_2_0_si_riscrive_una_volta_col_segno`).
    fn con_il_segno(mut contenuto: Contenuto) -> Contenuto {
        contenuto
            .capacita
            .extend(documento::capacita_di_questa_versione());
        contenuto
    }

    /// Un giorno, in millisecondi: più del margine con cui si accetta un
    /// orologio avanti.
    const UN_GIORNO: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn quel_che_esce_dal_dominio_non_entra_nella_fusione() {
        // Un documento scritto a mano nella cartella condivisa: un voto da sei
        // stelle, che il `CHECK` di `tracks.rating` rifiuta e che faceva fallire
        // ogni passata; un conteggio presso `i64::MAX`, che fa traboccare la
        // `SUM` della libreria; un cuore e una lapide dal futuro, che vincevano
        // per sempre su ogni decisione presa qui.
        let adesso = 1_700_000_000_000;
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut storto = Contenuto::default();
        storto.voti.insert("sei".to_owned(), Voto { v: 6, at: 10 });
        storto.voti.insert("tre".to_owned(), Voto { v: 3, at: 10 });
        storto.ascolti.insert("tanti".to_owned(), i64::MAX - 1);
        storto.ereditati.insert("tanti".to_owned(), i64::MAX - 1);
        storto.ascolti.insert("pochi".to_owned(), 4);
        storto.preferiti.insert(
            "domani".to_owned(),
            crate::registro::Interruttore {
                on: true,
                at: adesso + UN_GIORNO,
            },
        );
        storto
            .lapidi
            .playlist
            .insert("preferite".to_owned(), i64::MAX);
        let byte =
            documento::serializza(&Documento::nuovo("altro", storto, adesso)).expect("serializza");
        magazzino
            .scrivi(&documento::nome_file("altro"), None, TIPO, &byte, "x")
            .expect("scrive");

        let mut memoria = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "io", &mut memoria);
        let esito = motore
            .passata(&Contenuto::default(), adesso, &niente)
            .expect("passata");

        assert_eq!(esito.fuso.voti.get("sei"), None, "sei stelle non esistono");
        assert_eq!(esito.fuso.voti.get("tre").map(|v| v.v), Some(3));
        assert_eq!(esito.fuso.ascolti_di("tanti"), 0, "fuori scala");
        assert_eq!(esito.fuso.ascolti_di("pochi"), 4);
        assert!(!esito.fuso.preferiti.contains_key("domani"), "rinviato");
        assert!(
            esito.fuso.lapidi.playlist.is_empty(),
            "una lapide dal futuro"
        );

        // Quando il suo momento arriva il cuore entra, e il documento è lo
        // stesso di prima: rinviato, non perso.
        let esito = motore
            .passata(&Contenuto::default(), adesso + 2 * UN_GIORNO, &niente)
            .expect("passata");
        assert!(esito.fuso.preferiti.get("domani").is_some_and(|c| c.on));
    }

    fn nomi(magazzino: &Cartella) -> Vec<String> {
        magazzino
            .elenca()
            .expect("elenca")
            .into_iter()
            .map(|file| file.nome)
            .collect()
    }

    /// Un magazzino che tiene le impronte, come Drive, e conta le letture.
    struct ConImpronte {
        cartella: Cartella,
        letture: std::sync::atomic::AtomicUsize,
    }

    impl Magazzino for ConImpronte {
        fn elenca(&self) -> Result<Vec<FileRemoto>, AppError> {
            Ok(self
                .cartella
                .elenca()?
                .into_iter()
                .map(|file| FileRemoto {
                    impronta: Some(format!("sempre-la-stessa-{}", file.nome)),
                    ..file
                })
                .collect())
        }
        fn leggi(&self, id: &str) -> Result<Vec<u8>, AppError> {
            self.letture
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.cartella.leggi(id)
        }
        fn scrivi(
            &self,
            nome: &str,
            esistente: Option<&str>,
            tipo: &str,
            dati: &[u8],
            impronta: &str,
        ) -> Result<FileRemoto, AppError> {
            self.cartella.scrivi(nome, esistente, tipo, dati, impronta)
        }
        fn rinomina(&self, id: &str, nome: &str) -> Result<(), AppError> {
            self.cartella.rinomina(id, nome)
        }
        fn cancella(&self, id: &str) -> Result<(), AppError> {
            self.cartella.cancella(id)
        }
    }

    #[test]
    fn un_documento_guasto_e_immutato_non_si_rilegge_a_ogni_passata() {
        // Un documento che non si interpreta finiva fra i guasti e non nella
        // memoria: alla passata dopo «immutato» era falso e lo si riscaricava e
        // riapriva, ogni volta. Il guasto si dice ancora, ogni volta.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = ConImpronte {
            cartella: Cartella::nuova(dove.path().join("dispositivi")),
            letture: std::sync::atomic::AtomicUsize::new(0),
        };
        magazzino
            .cartella
            .scrivi(
                &documento::nome_file("rotto"),
                None,
                TIPO,
                b"non sono un gzip",
                "x",
            )
            .expect("scrive");

        let mut memoria = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "io", &mut memoria);
        for _ in 0..3 {
            let esito = motore
                .passata(&Contenuto::default(), 1_000, &niente)
                .expect("passata");
            assert_eq!(esito.passata.guasti.len(), 1, "il guasto si dice");
        }
        assert_eq!(
            magazzino.letture.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "letto una volta sola"
        );
    }

    /// Un magazzino che non riesce né a rinominare né a cancellare: lo stato di
    /// una cartella in sola lettura, o di un Drive che risponde male.
    struct SenzaRinomina(Cartella);

    impl Magazzino for SenzaRinomina {
        fn elenca(&self) -> Result<Vec<FileRemoto>, AppError> {
            self.0.elenca()
        }
        fn leggi(&self, id: &str) -> Result<Vec<u8>, AppError> {
            self.0.leggi(id)
        }
        fn scrivi(
            &self,
            nome: &str,
            esistente: Option<&str>,
            tipo: &str,
            dati: &[u8],
            impronta: &str,
        ) -> Result<FileRemoto, AppError> {
            self.0.scrivi(nome, esistente, tipo, dati, impronta)
        }
        fn rinomina(&self, _id: &str, _nome: &str) -> Result<(), AppError> {
            Err(AppError::new(
                rymembra_domain::errors::ErrorCode::NetOffline { url: None },
            ))
        }
        fn cancella(&self, _id: &str) -> Result<(), AppError> {
            Err(AppError::new(
                rymembra_domain::errors::ErrorCode::NetOffline { url: None },
            ))
        }
    }

    #[test]
    fn un_documento_altrui_col_nome_di_prima_si_legge() {
        // Il telefono non si è ancora aggiornato e scrive ancora col nome di
        // prima: i suoi voti devono arrivare lo stesso, e il suo file non è
        // nostro da toccare.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        scrivi_di_prima(
            &magazzino,
            "telefono",
            con_voto("so-what", Voto { v: 5, at: 100 }),
        );

        let mut memoria = Memoria::nuova();
        let mut portatile = Motore::nuovo(&magazzino, "portatile", &mut memoria);
        let esito = portatile
            .passata(&Contenuto::default(), 2_000, &niente)
            .expect("passata");

        assert_eq!(esito.passata.letti, 1);
        assert_eq!(esito.fuso.voti.get("so-what").map(|v| v.v), Some(5));
        assert!(esito.fuso.dispositivi.contains(&"telefono".to_owned()));
        assert!(
            nomi(&magazzino).contains(&nome_di_prima("telefono")),
            "il documento di un altro non si rinomina: lo fa lui quando si aggiorna"
        );
    }

    #[test]
    fn il_proprio_documento_di_prima_si_rinomina_e_non_e_un_pari() {
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut mio = Contenuto::default();
        mio.ascolti.insert("so-what".to_owned(), 5);
        mio.voti.insert("so-what".to_owned(), Voto { v: 4, at: 10 });
        scrivi_di_prima(&magazzino, "portatile", con_il_segno(mio.clone()));

        let mut memoria = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "portatile", &mut memoria);
        let esito = motore.passata(&mio, 2_000, &niente).expect("passata");

        assert_eq!(
            nomi(&magazzino),
            vec![documento::nome_file("portatile")],
            "il documento ha preso il nome di adesso e non ce n'è un altro"
        );
        assert_eq!(esito.passata.letti, 0, "non si è letto come un pari");
        assert_eq!(
            esito.fuso.ascolti_di("so-what"),
            5,
            "gli ascolti propri non si contano due volte"
        );
        assert_eq!(esito.fuso.dispositivi, vec!["portatile".to_owned()]);
        assert!(
            !esito.passata.scritto,
            "il contenuto era già quello: la rinomina basta, niente riscrittura"
        );
        // E il file rinominato è proprio quello di prima, con i suoi byte.
        let riletto = magazzino
            .leggi(&documento::nome_file("portatile"))
            .and_then(|byte| documento::interpreta(&byte))
            .expect("rileggibile");
        assert_eq!(riletto.content, con_il_segno(mio));
    }

    #[test]
    fn se_ci_sono_il_nuovo_e_il_vecchio_proprio_il_vecchio_si_toglie() {
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        let mut mio = Contenuto::default();
        mio.ascolti.insert("so-what".to_owned(), 5);
        // Il vecchio è rimasto indietro; il nuovo è quello scritto da questo
        // dispositivo dopo l'aggiornamento.
        let mut indietro = Contenuto::default();
        indietro.ascolti.insert("so-what".to_owned(), 2);
        scrivi_di_prima(&magazzino, "portatile", indietro);
        let mut memoria_prima = Memoria::nuova();
        Motore::nuovo(&magazzino, "portatile", &mut memoria_prima)
            .passata(&mio, 1_000, &niente)
            .expect("prima");
        // La prima passata ha già sistemato tutto: rimettiamo il vecchio per
        // simulare una rinomina arrivata in ritardo dalla cartella condivisa.
        scrivi_di_prima(&magazzino, "portatile", Contenuto::default());
        assert_eq!(nomi(&magazzino).len(), 2);

        let mut memoria = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "portatile", &mut memoria);
        let esito = motore.passata(&mio, 2_000, &niente).expect("passata");

        assert_eq!(nomi(&magazzino), vec![documento::nome_file("portatile")]);
        assert_eq!(esito.passata.letti, 0);
        assert_eq!(esito.fuso.ascolti_di("so-what"), 5);
        assert!(!esito.passata.scritto, "il nuovo era già a posto");
    }

    #[test]
    fn un_rinomino_del_proprio_che_fallisce_non_ferma_la_passata() {
        // Se la rinomina e la cancellazione non riescono il documento di prima
        // resta lì, ma la passata va avanti, non lo conta come un pari e
        // scrive il documento nuovo.
        let dove = tempfile::tempdir().expect("cartella");
        let cartella = Cartella::nuova(dove.path().join("dispositivi"));
        let mut mio = Contenuto::default();
        mio.ascolti.insert("so-what".to_owned(), 5);
        scrivi_di_prima(&cartella, "portatile", con_il_segno(mio.clone()));
        let magazzino = SenzaRinomina(cartella.clone());

        let mut memoria = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "portatile", &mut memoria);
        let esito = motore.passata(&mio, 2_000, &niente).expect("passata");

        assert_eq!(esito.passata.letti, 0, "mai letto come un pari");
        assert!(esito.passata.guasti.is_empty());
        assert_eq!(esito.fuso.ascolti_di("so-what"), 5);
        assert!(
            !esito.passata.scritto,
            "l'impronta si legge dal documento di prima, che è identico"
        );

        // Un contenuto cambiato si scrive sotto il nome di adesso.
        mio.ascolti.insert("so-what".to_owned(), 6);
        let dopo = motore.passata(&mio, 3_000, &niente).expect("passata");
        assert!(dopo.passata.scritto);
        assert!(nomi(&cartella).contains(&documento::nome_file("portatile")));
        assert_eq!(dopo.fuso.ascolti_di("so-what"), 6);
        assert_eq!(dopo.passata.letti, 0);
    }

    #[test]
    fn il_documento_di_prima_di_un_altro_dispositivo_con_un_nome_simile_non_e_il_mio() {
        // «portatile» e «portatile2» non si confondono: si confronta l'intero
        // identificativo, non un prefisso.
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = Cartella::nuova(dove.path().join("dispositivi"));
        scrivi_di_prima(
            &magazzino,
            "portatile2",
            con_voto("a", Voto { v: 2, at: 5 }),
        );

        let mut memoria = Memoria::nuova();
        let mut motore = Motore::nuovo(&magazzino, "portatile", &mut memoria);
        let esito = motore
            .passata(&Contenuto::default(), 2_000, &niente)
            .expect("passata");
        assert_eq!(esito.passata.letti, 1);
        assert_eq!(esito.fuso.voti.get("a").map(|v| v.v), Some(2));
        assert!(nomi(&magazzino).contains(&nome_di_prima("portatile2")));
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
