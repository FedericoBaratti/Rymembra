//! Quando riallineare quale cartella: il [`Pianificatore`], puro, con l'orologio
//! passato da fuori. Fa parte di [`crate::sorveglia`]: i tempi stanno là.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

use super::{RESPIRO, Ritmo, SONDAGGIO, SONNO_MARGINE};

// ── il pianificatore dei riallineamenti ─────────────────────────────────────

/// Un riallineamento da fare adesso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Riallineamento {
    /// La cartella sorvegliata.
    pub radice: String,
    /// Prima di camminarla si rifà l'osservatore.
    ///
    /// Prima e non dopo: quel che cambia mentre si cammina lo vede già quello
    /// nuovo. E rifarlo è l'unico modo di sapere che quello vecchio non era
    /// sordo, perché un osservatore di rete sordo non lo dice (vedi il
    /// preambolo di `sorveglia.rs`).
    pub riarma: bool,
}

/// Quel che il pianificatore sa di una cartella.
#[derive(Debug, Clone)]
struct Programma {
    /// Il passo, come l'ha detto l'ultima configurazione.
    ritmo: Ritmo,
    /// Va riallineata appena si può.
    dovuta: bool,
    /// E prima si rifà l'osservatore.
    riarma: bool,
    /// Il prossimo riallineamento periodico, o il rinvio di uno che non ha
    /// risposto.
    prossimo: Option<Instant>,
}

/// Quando riallineare quale cartella.
///
/// Puro, come l'[`Accorpatore`]: l'orologio arriva da fuori, e le prove gli
/// raccontano mezz'ore e sospensioni senza aspettarle. Il filo lo informa dei
/// fatti — com'è stata armata una cartella, il risveglio finito, un'attesa
/// durata troppo — e gli chiede ogni volta [`Self::prossima`].
///
/// # Le cause
///
/// - **L'avvio**, dopo il risveglio delle cartelle di rete: per le cartelle
///   che erano nell'elenco quando il filo è partito ([`Self::fotografa`]).
///   Quello che si è perso mentre Rymembra era chiuso non ha mandato avvisi a
///   nessuno. Il primo riallineamento aspetta [`Self::avvio`] perché un NAS
///   addormentato, interrogato prima di essersi svegliato, risponderebbe «non
///   ci sono» e la cartella risulterebbe saltata.
/// - **Il risveglio dal sospeso**, visto da [`Self::sveglio`]: tutte, e con
///   l'osservatore rifatto prima — locali comprese.
/// - **Il ritorno** di una cartella di rete che per un po' non ha risposto
///   ([`Self::tornata`]).
/// - **Ogni cambio di passo** ([`Self::imposta`]): una cartella che passa da
///   non guardata a guardata è stata un po' senza nessuno a guardarla — un
///   disco staccato e riattaccato, un osservatore guastato e rifatto, la
///   sorveglianza riaccesa.
/// - **Il periodico**, con l'osservatore rifatto prima: ogni [`RIALLINEAMENTO`]
///   per le cartelle di rete con gli avvisi, ogni [`RIALLINEAMENTO_LOCALE`] per
///   quelle di questo computer; ogni [`SONDAGGIO`] per quelle col solo
///   riallineamento, che un osservatore non ce l'hanno.
///
/// Più cause insieme fanno un riallineamento solo. Una cartella **aggiunta**
/// mentre Rymembra è aperto non si riallinea al primo armo: chi la aggiunge la
/// scansiona — il primo avvio lo fa da sé — e una lettura intera partita da
/// sola terrebbe il turno proprio a quella «Scansiona». Vale anche per quella
/// scelta nei primi secondi, prima che il filo abbia letto l'elenco: è il primo
/// avvio, e la sua «Scansiona» parte subito.
///
/// [`Accorpatore`]: super::Accorpatore
/// [`RIALLINEAMENTO`]: super::RIALLINEAMENTO
/// [`RIALLINEAMENTO_LOCALE`]: super::RIALLINEAMENTO_LOCALE
#[derive(Debug, Default)]
pub struct Pianificatore {
    /// Le cartelle, per nome.
    radici: BTreeMap<String, Programma>,
    /// Il risveglio delle cartelle di rete all'avvio è finito: da qui si
    /// riallinea.
    avviato: bool,
    /// L'elenco delle cartelle è stato letto almeno una volta: quelle che
    /// arrivano dopo sono aggiunte, non ritrovate.
    elenco_letto: bool,
    /// Le cartelle dell'elenco quando il filo è partito: solo loro hanno il
    /// riallineamento dell'avvio. `None` se non si sono potute leggere, e
    /// allora vale il primo elenco letto. Vedi [`Self::fotografa`].
    dell_avvio: Option<Vec<String>>,
    /// Niente prima di questo istante: il turno era occupato, o una scansione
    /// automatica è appena finita.
    non_prima: Option<Instant>,
    /// L'attesa a vuoto in corso: l'orologio di sistema quando è cominciata, e
    /// quanto si è chiesto di aspettare.
    sonno: Option<(SystemTime, Duration)>,
}

impl Pianificatore {
    /// Il filo ha appena (ri)configurato una cartella, e questo è il suo passo.
    ///
    /// Una cartella nuova si riallinea se arriva con il primo elenco letto ed
    /// era già nell'elenco quando il filo è partito; una già nota se il passo
    /// cambia verso uno guardato. Un passo uguale — un osservatore rifatto per
    /// il periodico, un ripiego che riprova e non ce la fa — non è una causa: è
    /// già dovuta, o non è cambiato niente.
    pub fn imposta(&mut self, radice: &str, ritmo: Ritmo, adesso: Instant) {
        let prossimo = ritmo.periodo().map(|periodo| adesso + periodo);
        let Some(programma) = self.radici.get_mut(radice) else {
            let dell_avvio = !self.elenco_letto
                && self
                    .dell_avvio
                    .as_ref()
                    .is_none_or(|elenco| elenco.iter().any(|gia| gia == radice));
            self.radici.insert(
                radice.to_owned(),
                Programma {
                    ritmo,
                    dovuta: dell_avvio && ritmo.guardata(),
                    riarma: false,
                    prossimo,
                },
            );
            return;
        };
        // È appena stata rifatta: un riarmo in sospeso è fatto.
        programma.riarma = false;
        if programma.ritmo == ritmo {
            return;
        }
        programma.ritmo = ritmo;
        programma.dovuta = ritmo.guardata();
        programma.prossimo = prossimo;
    }

    /// Dimentica le cartelle che non sono più nell'elenco.
    pub fn tieni_solo(&mut self, radici: &[String]) {
        self.radici.retain(|radice, _| radici.contains(radice));
    }

    /// L'elenco com'era quando il filo è partito: solo queste cartelle hanno
    /// il riallineamento dell'avvio.
    ///
    /// Si fotografa prima che chiunque possa aggiungerne. Il filo legge
    /// l'elenco la prima volta dopo `ATTESA_AVVIO`, o prima se qualcuno lo
    /// cambia; e al primo avvio è proprio quello che succede — la cartella
    /// scelta nei primi secondi arrivava con il primo elenco letto, si
    /// riallineava come se fosse stata lì da sempre, cioè si importava da
    /// sola, senza avanzamento, e la «Scansiona» partita insieme aspettava la
    /// sua fine e ricamminava tutto.
    pub fn fotografa(&mut self, radici: Vec<String>) {
        self.dell_avvio = Some(radici);
    }

    /// L'elenco è stato letto: da qui una cartella nuova è un'aggiunta.
    pub fn elenco_letto(&mut self) {
        self.elenco_letto = true;
        self.dell_avvio = None;
    }

    /// Il risveglio delle cartelle di rete è finito: i riallineamenti possono
    /// cominciare.
    pub fn avvio(&mut self) {
        self.avviato = true;
    }

    /// Il filo sta per aspettare a vuoto `chiesta`; `muro` è l'orologio di
    /// sistema adesso.
    pub fn dormo(&mut self, chiesta: Duration, muro: SystemTime) {
        self.sonno = Some((muro, chiesta));
    }

    /// L'attesa è finita; `muro` è l'orologio di sistema adesso. `true` se è
    /// durata tanto da essere un sospeso, e allora si riallinea tutto.
    ///
    /// # Perché solo l'attesa
    ///
    /// Perché è l'unico tratto di cui si sa quanto doveva durare. Una
    /// scansione può durare dieci minuti su un NAS lento, e misurata col muro
    /// sembrerebbe un sonno; un'attesa di un minuto che ne dura tre no. Per
    /// questo la misura comincia a [`Self::dormo`] e qui si consuma: quel che
    /// succede fra un'attesa e l'altra non entra mai nel conto.
    ///
    /// E l'orologio di sistema, non `Instant`: su Windows non è garantito che
    /// `Instant` conti il tempo passato in sospensione, e confrontare i due
    /// sarebbe misurare la differenza fra due orologi che non si sa come
    /// contano. Un orologio portato indietro durante l'attesa non è un sonno; uno
    /// portato avanti di ore sì, e costa un riallineamento in più.
    pub fn sveglio(&mut self, muro: SystemTime) -> bool {
        let Some((inizio, chiesta)) = self.sonno.take() else {
            return false;
        };
        let Ok(trascorso) = muro.duration_since(inizio) else {
            return false;
        };
        if trascorso <= chiesta.saturating_mul(2).saturating_add(SONNO_MARGINE) {
            return false;
        }
        for programma in self.radici.values_mut() {
            if programma.ritmo.guardata() {
                programma.dovuta = true;
                // Un osservatore di rete non sopravvive a un sonno lungo, e se
                // è morto non lo dice. Uno locale di solito sopravvive, ma se
                // non ce l'ha fatta non lo dice nemmeno lui: rifarlo costa un
                // attimo, tenerne uno sordo fino al giro dopo costa un'ora.
                programma.riarma |= programma.ritmo.con_osservatore();
            }
        }
        true
    }

    /// Una cartella di rete torna a rispondere dopo un po' di silenzio: il suo
    /// osservatore è probabilmente morto con la rete, e quel che è cambiato
    /// intanto non l'ha visto nessuno.
    pub fn tornata(&mut self, radice: &str) {
        if let Some(programma) = self.radici.get_mut(radice)
            && programma.ritmo.guardata()
        {
            programma.dovuta = true;
            programma.riarma |= programma.ritmo == Ritmo::Rete;
        }
    }

    /// Il prossimo riallineamento da fare adesso, se ce n'è uno e se si può.
    ///
    /// Uno alla volta, nell'ordine delle cartelle: fra uno e l'altro il filo
    /// legge la sua posta e lascia un [`RESPIRO`] a chi aspetta il turno.
    pub fn prossima(&mut self, adesso: Instant) -> Option<Riallineamento> {
        // I periodici scaduti diventano dovuti. Il passo si azzera qui e non
        // alla fine: se la cartella non si può riallineare subito — il turno è
        // occupato — non deve maturare di nuovo, né chiedere un altro riarmo.
        for programma in self.radici.values_mut() {
            if programma.ritmo.guardata()
                && programma.prossimo.is_some_and(|quando| quando <= adesso)
            {
                programma.dovuta = true;
                programma.riarma |= programma.ritmo.con_osservatore();
                programma.prossimo = None;
            }
        }
        if !self.libero(adesso) {
            return None;
        }
        self.radici
            .iter()
            .find(|(_, programma)| programma.dovuta && programma.ritmo.guardata())
            .map(|(radice, programma)| Riallineamento {
                radice: radice.clone(),
                riarma: programma.riarma,
            })
    }

    /// Si può cominciare una scansione automatica: l'avvio è fatto, e il
    /// [`RESPIRO`] è passato.
    #[must_use]
    pub fn libero(&self, adesso: Instant) -> bool {
        self.avviato && self.fra_quanto_libero(adesso).is_zero()
    }

    /// Quanto manca alla fine del [`RESPIRO`].
    #[must_use]
    pub fn fra_quanto_libero(&self, adesso: Instant) -> Duration {
        self.non_prima.map_or(Duration::ZERO, |quando| {
            quando.saturating_duration_since(adesso)
        })
    }

    /// Niente per un [`RESPIRO`]: il turno era occupato, o una scansione
    /// automatica è appena finita.
    pub fn aspetta_un_respiro(&mut self, adesso: Instant) {
        self.non_prima = Some(adesso + RESPIRO);
    }

    /// Il riallineamento di `radice` è finito; `risposto` se la cartella ha
    /// risposto: guardata tutta, o trovata vuota dalla guardia per radice.
    ///
    /// Vuota vale risposto, e resta al suo passo: una cartella che risponde
    /// vuota risponderà vuota anche fra cinque minuti, e riprovarla a ogni
    /// [`SONDAGGIO`] era un riallineamento ogni cinque minuti per sempre su una
    /// cartella svuotata davvero. La riguardano il suo periodico, il risveglio,
    /// il ritorno — o una «Scansiona», dove c'è chi può confermare.
    ///
    /// Una cartella che non ha risposto resta in debito, e si riprova al
    /// prossimo motivo — il ritorno, il risveglio, un riarmo — o al più fra
    /// [`SONDAGGIO`]: mai subito, che su un NAS spento sarebbe un ciclo stretto
    /// di sonde che scadono. Anche una cartella locale, che di periodico ne ha
    /// uno lungo: un disco esterno che gira ancora a vuoto non deve aspettare
    /// un'ora.
    pub fn fatto(&mut self, radice: &str, risposto: bool, adesso: Instant) {
        self.aspetta_un_respiro(adesso);
        let Some(programma) = self.radici.get_mut(radice) else {
            return;
        };
        programma.dovuta = false;
        programma.riarma = false;
        programma.prossimo = if !programma.ritmo.guardata() {
            None
        } else if risposto {
            programma.ritmo.periodo().map(|periodo| adesso + periodo)
        } else {
            let rinvio = programma
                .ritmo
                .periodo()
                .map_or(SONDAGGIO, |periodo| periodo.min(SONDAGGIO));
            Some(adesso + rinvio)
        };
    }

    /// Quanto manca al prossimo riallineamento. `None` se non ce n'è nessuno
    /// in vista — o se l'avvio non è ancora arrivato: lo porta un messaggio,
    /// che sveglia il filo da sé.
    #[must_use]
    pub fn manca(&self, adesso: Instant) -> Option<Duration> {
        if !self.avviato {
            return None;
        }
        let primo = self
            .radici
            .values()
            .filter(|programma| programma.ritmo.guardata())
            .filter_map(|programma| {
                if programma.dovuta {
                    Some(Duration::ZERO)
                } else {
                    programma
                        .prossimo
                        .map(|quando| quando.saturating_duration_since(adesso))
                }
            })
            .min()?;
        Some(primo.max(self.fra_quanto_libero(adesso)))
    }
}
