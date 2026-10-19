//! L'ordine di una playlist, fuso senza che nessuno debba rinunciare al proprio.
//!
//! # Il caso che oggi si perde
//!
//! `restore.rs` dice la verità quando scrive che «non esiste una mezza fusione
//! difendibile di un elenco ordinato», e da lì conclude che una playlist si
//! prende intera dal dispositivo che l'ha toccata per ultima. Il prezzo lo si
//! paga in un caso solo, ma è un caso che capita: due dispositivi aggiungono un
//! brano alla stessa playlist mentre sono scollegati, e alla passata dopo uno dei
//! due brani non c'è più. Nessun errore, nessun avviso: semplicemente non c'è.
//!
//! Questo modulo è la risposta a quella frase. Non fonde due *elenchi* — quello è
//! davvero indecidibile — ma tiene una struttura da cui l'elenco si **ricava**, e
//! che di fusioni ne ammette una sola possibile.
//!
//! # Come funziona, in tre righe
//!
//! Ogni brano nella playlist è un elemento con un'identità sua
//! (`<dispositivo>:<numero>`) e un puntatore a **dopo chi** è stato inserito.
//! L'elenco non è memorizzato: è la visita di quell'albero, dalla testa, con i
//! figli di ogni nodo in ordine di identità decrescente. Due dispositivi che
//! inseriscono dopo lo stesso brano producono due figli dello stesso nodo, e la
//! visita li mette in fila entrambi, sempre nello stesso ordine su tutti e due.
//!
//! ```text
//!   (testa)
//!     ├── telefono:3  «Blue in Green»       ← inserito mentre erano scollegati
//!     └── portatile:3 «So What»             ← anche questo
//!           └── portatile:4 «Flamenco Sketches»
//! ```
//!
//! # Perché una lapide e non un buco
//!
//! Togliere un elemento davvero vorrebbe dire che l'altro dispositivo, che non ha
//! visto la rimozione, lo rimanda indietro alla prima passata — lo stesso difetto
//! delle playlist cancellate che `Lapidi` esiste per evitare. Un elemento tolto
//! resta quindi nella struttura con la data in cui è stato tolto, e sparisce solo
//! dall'elenco che si ricava. Rimettere lo stesso brano crea un elemento
//! **nuovo**, e non è un dettaglio: è ciò che permette a una playlist di avere due
//! volte lo stesso brano, cosa che `playlist_tracks` ammette già.
//!
//! # Cosa NON c'è, e non è una dimenticanza
//!
//! Non c'è la raccolta delle lapidi. Prima o poi una playlist molto rimaneggiata
//! porta più elementi tolti che presenti, e a quel punto si vorrebbe potarli — ma
//! potare vuol dire decidere che tutti i dispositivi hanno visto abbastanza, e
//! senza un server quella domanda non ha risposta. Il costo è una riga di JSON per
//! brano tolto: su una playlist rifatta cento volte sono decine di chilobyte,
//! compressi in poche centinaia di byte. Si paga.

use std::collections::{BTreeMap, BTreeSet};

use crate::registro::Registro;

/// Chi ha inserito un elemento, e il quantesimo è stato.
///
/// L'ordine è per **numero** e poi per dispositivo, non il contrario: due
/// inserimenti concorrenti hanno lo stesso numero, e il nome del dispositivo
/// serve solo a spareggiarli. Ordinare prima per dispositivo vorrebbe dire che
/// tutto ciò che fa un computer che si chiama `AAA…` precede tutto ciò che fa
/// un `ZZZ…`, indipendentemente da quando.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identita {
    /// Il quantesimo elemento è, nel contatore di chi lo ha inserito.
    pub n: u64,
    /// Chi lo ha inserito.
    pub dispositivo: String,
}

impl Identita {
    /// Un'identità nuova.
    #[must_use]
    pub fn nuova(dispositivo: &str, n: u64) -> Self {
        Self {
            n,
            dispositivo: dispositivo.to_owned(),
        }
    }

    /// Come si scrive nel documento.
    #[must_use]
    pub fn testo(&self) -> String {
        format!("{}:{}", self.dispositivo, self.n)
    }

    /// Come si rilegge.
    ///
    /// `None` per tutto ciò che non ha la forma attesa: un elemento con
    /// un'identità illeggibile si butta, il documento no. È la stessa indulgenza
    /// che `backup::interpreta` applica ai suoi record.
    #[must_use]
    pub fn da_testo(grezzo: &str) -> Option<Self> {
        let (dispositivo, numero) = grezzo.rsplit_once(':')?;
        if dispositivo.is_empty() {
            return None;
        }
        Some(Self {
            n: numero.parse().ok()?,
            dispositivo: dispositivo.to_owned(),
        })
    }
}

impl serde::Serialize for Identita {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.testo())
    }
}

impl<'de> serde::Deserialize<'de> for Identita {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let grezzo = String::deserialize(deserializer)?;
        Self::da_testo(&grezzo).ok_or_else(|| {
            serde::de::Error::custom(format!("identità di sequenza illeggibile: {grezzo}"))
        })
    }
}

/// Un brano nella playlist, con dove è stato messo e se è ancora lì.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Elemento {
    /// Dopo quale elemento è stato inserito. `None` vuol dire in testa.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dopo: Option<Identita>,
    /// La chiave del brano.
    pub brano: String,
    /// Quando è stato tolto, se è stato tolto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolto: Option<i64>,
}

/// L'ordine di una playlist.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Sequenza {
    /// Gli elementi, presenti e tolti, in ordine di identità.
    elementi: BTreeMap<Identita, Elemento>,
}

impl Sequenza {
    /// Una playlist vuota.
    #[must_use]
    pub fn vuota() -> Self {
        Self::default()
    }

    /// Una playlist costruita da un elenco, come catena.
    ///
    /// È il punto d'ingresso: una playlist che esisteva prima che ci fosse una
    /// sincronia, o che arriva da un'importazione, entra da qui.
    #[must_use]
    pub fn dalla_lista(dispositivo: &str, brani: &[String]) -> Self {
        let mut sequenza = Self::vuota();
        let mut precedente = None;
        for (indice, brano) in brani.iter().enumerate() {
            let identita = Identita::nuova(dispositivo, u64::try_from(indice).unwrap_or(u64::MAX));
            sequenza.elementi.insert(
                identita.clone(),
                Elemento {
                    dopo: precedente,
                    brano: brano.clone(),
                    tolto: None,
                },
            );
            precedente = Some(identita);
        }
        sequenza
    }

    /// L'elenco che si ricava: i brani presenti, nell'ordine.
    ///
    /// La visita è iterativa e non ricorsiva di proposito: una playlist da
    /// cinquemila brani è una catena profonda cinquemila, e una ricorsione così
    /// esaurisce la pila — cioè fa cadere l'app di chi ha una playlist grande,
    /// che è precisamente chi non se lo può permettere.
    #[must_use]
    pub fn ordine(&self) -> Vec<&str> {
        let mut figli: BTreeMap<Option<&Identita>, Vec<&Identita>> = BTreeMap::new();
        for (identita, elemento) in &self.elementi {
            figli
                .entry(elemento.dopo.as_ref())
                .or_default()
                .push(identita);
        }

        let mut elenco = Vec::with_capacity(self.elementi.len());
        let mut visitati: BTreeSet<&Identita> = BTreeSet::new();
        let mut pila: Vec<&Identita> = figli.get(&None).cloned().unwrap_or_default();

        // I figli arrivano in ordine crescente dalla `BTreeMap`; impilandoli così
        // la `pop` li restituisce decrescenti, che è l'ordine in cui due
        // inserimenti concorrenti sullo stesso ancoraggio devono comparire.
        while let Some(identita) = pila.pop() {
            if !visitati.insert(identita) {
                continue;
            }
            if let Some(elemento) = self.elementi.get(identita)
                && elemento.tolto.is_none()
            {
                elenco.push(elemento.brano.as_str());
            }
            if let Some(discendenti) = figli.get(&Some(identita)) {
                pila.extend(discendenti.iter().copied());
            }
        }

        // Gli orfani: elementi che puntano a un ancoraggio che non abbiamo. Capita
        // per davvero, e non è corruzione — il documento del dispositivo che ha
        // scritto l'ancoraggio può non essere ancora arrivato. Si accodano in
        // ordine di identità: è una posizione arbitraria, ma è la stessa su tutti
        // i dispositivi, ed è comunque meglio di perderli.
        for (identita, elemento) in &self.elementi {
            if !visitati.contains(identita) && elemento.tolto.is_none() {
                elenco.push(elemento.brano.as_str());
            }
        }
        elenco
    }

    /// Il numero da dare al prossimo elemento.
    ///
    /// Il massimo visto più uno, e non un contatore da conservare: un contatore
    /// separato è un secondo stato da tenere allineato al primo, e il giorno che
    /// diverge produce due elementi con la stessa identità — cioè due brani che si
    /// mangiano a vicenda. Il massimo lo si ricava sempre, e non può sbagliare.
    fn prossimo(&self) -> u64 {
        self.elementi
            .keys()
            .last()
            .map_or(0, |identita| identita.n.saturating_add(1))
    }

    /// Inserisce un brano dopo un elemento, o in testa.
    fn metti(&mut self, dispositivo: &str, dopo: Option<Identita>, brano: &str) -> Identita {
        let identita = Identita::nuova(dispositivo, self.prossimo());
        self.elementi.insert(
            identita.clone(),
            Elemento {
                dopo,
                brano: brano.to_owned(),
                tolto: None,
            },
        );
        identita
    }

    /// Le identità presenti, nell'ordine dell'elenco.
    fn identita_in_ordine(&self) -> Vec<Identita> {
        let mut figli: BTreeMap<Option<&Identita>, Vec<&Identita>> = BTreeMap::new();
        for (identita, elemento) in &self.elementi {
            figli
                .entry(elemento.dopo.as_ref())
                .or_default()
                .push(identita);
        }
        let mut elenco = Vec::with_capacity(self.elementi.len());
        let mut visitati: BTreeSet<&Identita> = BTreeSet::new();
        let mut pila: Vec<&Identita> = figli.get(&None).cloned().unwrap_or_default();
        while let Some(identita) = pila.pop() {
            if !visitati.insert(identita) {
                continue;
            }
            if let Some(elemento) = self.elementi.get(identita)
                && elemento.tolto.is_none()
            {
                elenco.push(identita.clone());
            }
            if let Some(discendenti) = figli.get(&Some(identita)) {
                pila.extend(discendenti.iter().copied());
            }
        }
        for (identita, elemento) in &self.elementi {
            if !visitati.contains(identita) && elemento.tolto.is_none() {
                elenco.push(identita.clone());
            }
        }
        elenco
    }

    /// Aggiunge un brano in coda.
    pub fn accoda(&mut self, dispositivo: &str, brano: &str) {
        let ultimo = self.identita_in_ordine().pop();
        self.metti(dispositivo, ultimo, brano);
    }

    /// Inserisce un brano a una posizione dell'elenco.
    ///
    /// Una posizione oltre la fine accoda: è quel che si aspetta chi trascina un
    /// brano in fondo, e rifiutare sarebbe rispondere a un gesto ragionevole con
    /// un errore.
    pub fn inserisci(&mut self, dispositivo: &str, posizione: usize, brano: &str) {
        let ordine = self.identita_in_ordine();
        let dopo = if posizione == 0 {
            None
        } else {
            // Oltre la fine si accoda, e non si torna in testa: `get` che
            // restituisce `None` per un indice troppo grande vorrebbe dire
            // «in testa», cioè l'opposto esatto di quel che ha chiesto chi ha
            // trascinato un brano in fondo.
            let indice = posizione.saturating_sub(1);
            ordine.get(indice).or_else(|| ordine.last()).cloned()
        };
        self.metti(dispositivo, dopo, brano);
    }

    /// Toglie il brano che sta a una posizione.
    ///
    /// Restituisce se c'era qualcosa da togliere.
    pub fn togli(&mut self, posizione: usize, quando: i64) -> bool {
        let Some(identita) = self.identita_in_ordine().get(posizione).cloned() else {
            return false;
        };
        if let Some(elemento) = self.elementi.get_mut(&identita) {
            elemento.tolto = Some(quando);
            return true;
        }
        false
    }

    /// Porta la playlist a un ordine nuovo.
    ///
    /// Non azzera e riscrive: passa in rassegna l'elenco attuale e quello voluto
    /// in parallelo, e tocca solo ciò che si è davvero mosso. Riscrivere tutto
    /// sarebbe più corto da scrivere e produrrebbe una lapide per brano a ogni
    /// trascinamento — su una playlist da trecento brani, trecento lapidi per aver
    /// spostato una canzone di un posto.
    pub fn riordina(&mut self, dispositivo: &str, voluto: &[String], quando: i64) {
        let attuale = self.identita_in_ordine();

        // Quel che non è più voluto se ne va. Si contano le occorrenze, perché lo
        // stesso brano può stare due volte nella stessa playlist.
        let mut quanti_ne_servono: BTreeMap<&str, usize> = BTreeMap::new();
        for brano in voluto {
            *quanti_ne_servono.entry(brano.as_str()).or_insert(0) += 1;
        }

        let mut sopravvissuti: Vec<Identita> = Vec::new();
        for identita in &attuale {
            let Some(elemento) = self.elementi.get(identita) else {
                continue;
            };
            let brano = elemento.brano.clone();
            match quanti_ne_servono.get_mut(brano.as_str()) {
                Some(quanti) if *quanti > 0 => {
                    *quanti -= 1;
                    sopravvissuti.push(identita.clone());
                }
                _ => {
                    if let Some(elemento) = self.elementi.get_mut(identita) {
                        elemento.tolto = Some(quando);
                    }
                }
            }
        }

        // Poi si allinea l'ordine: si scorre quel che si vuole tenendo un dito su
        // quel che c'è. Se combaciano si va avanti; se no, il brano voluto viene
        // staccato da dove sta e rimesso qui.
        let mut resto: Vec<Identita> = sopravvissuti;
        let mut precedente: Option<Identita> = None;
        for brano in voluto {
            let combacia = resto
                .first()
                .and_then(|identita| self.elementi.get(identita))
                .is_some_and(|elemento| elemento.brano == *brano);
            if combacia {
                precedente = resto.first().cloned();
                if !resto.is_empty() {
                    resto.remove(0);
                }
                continue;
            }
            // Si cerca più avanti un elemento con quel brano da riusare; se non
            // c'è, il brano è nuovo e si crea.
            let piu_avanti = resto.iter().position(|identita| {
                self.elementi
                    .get(identita)
                    .is_some_and(|elemento| elemento.brano == *brano)
            });
            if let Some(indice) = piu_avanti {
                let identita = resto.remove(indice);
                if let Some(elemento) = self.elementi.get_mut(&identita) {
                    elemento.tolto = Some(quando);
                }
            }
            precedente = Some(self.metti(dispositivo, precedente, brano));
        }
    }

    /// Quanti elementi in tutto, tolti compresi.
    #[must_use]
    pub fn quanti(&self) -> usize {
        self.elementi.len()
    }

    /// Nessun elemento, né presente né tolto.
    #[must_use]
    pub fn e_vuota(&self) -> bool {
        self.elementi.is_empty()
    }

    /// Gli elementi uno per uno, lapidi comprese.
    ///
    /// Serve a chi la deve scrivere su un database riga per riga. Le lapidi ci
    /// sono apposta: una sequenza salvata senza è una sequenza che dimentica le
    /// cancellazioni, e alla passata dopo i brani tolti tornano da soli.
    pub fn elementi(&self) -> impl Iterator<Item = (&Identita, &Elemento)> {
        self.elementi.iter()
    }

    /// Ricostruisce una sequenza dagli elementi salvati.
    ///
    /// L'inverso esatto di [`Sequenza::elementi`]: quel che esce da una passa da
    /// qui e torna identico, ed è la proprietà che rende il database un posto
    /// dove la sequenza può dormire fra due passate.
    #[must_use]
    pub fn dagli_elementi(elementi: impl IntoIterator<Item = (Identita, Elemento)>) -> Self {
        Self {
            elementi: elementi.into_iter().collect(),
        }
    }
}

impl Registro for Sequenza {
    fn fondi(&self, altro: &Self) -> Self {
        let mut fusa = self.clone();
        for (identita, elemento) in &altro.elementi {
            match fusa.elementi.get_mut(identita) {
                Some(gia) => {
                    // Stessa identità: per costruzione sono lo stesso elemento, e
                    // resta solo da mettere d'accordo la lapide. La più vecchia
                    // vince, perché il momento in cui una cosa è stata tolta la
                    // prima volta è un fatto e non un'opinione.
                    gia.tolto = match (gia.tolto, elemento.tolto) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (Some(a), None) => Some(a),
                        (None, Some(b)) => Some(b),
                        (None, None) => None,
                    };
                    // Se ancoraggio o brano divergono il documento è corrotto: non
                    // dovrebbe accadere, e succedendo si tiene il minore dei due
                    // per non dipendere da chi si chiama «locale».
                    if (&elemento.dopo, &elemento.brano) < (&gia.dopo, &gia.brano) {
                        gia.dopo.clone_from(&elemento.dopo);
                        gia.brano.clone_from(&elemento.brano);
                    }
                }
                None => {
                    fusa.elementi.insert(identita.clone(), elemento.clone());
                }
            }
        }
        fusa
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn lista(brani: &[&str]) -> Vec<String> {
        brani.iter().map(|b| (*b).to_owned()).collect()
    }

    #[test]
    fn una_lista_diventa_una_catena_e_torna_uguale() {
        let sequenza = Sequenza::dalla_lista("uno", &lista(&["a", "b", "c"]));
        assert_eq!(sequenza.ordine(), vec!["a", "b", "c"]);
    }

    #[test]
    fn due_aggiunte_concorrenti_sopravvivono_entrambe() {
        // Il caso che oggi si perde: due dispositivi scollegati aggiungono un
        // brano ciascuno alla stessa playlist, e con la regola «vince chi ha
        // toccato per ultimo» uno dei due sparisce senza dire niente.
        let base = Sequenza::dalla_lista("comune", &lista(&["a", "b"]));

        let mut portatile = base.clone();
        portatile.accoda("portatile", "so-what");

        let mut telefono = base.clone();
        telefono.accoda("telefono", "blue-in-green");

        let fusa = portatile.fondi(&telefono);
        let ordine = fusa.ordine();
        assert!(ordine.contains(&"so-what"), "manca il brano del portatile");
        assert!(
            ordine.contains(&"blue-in-green"),
            "manca il brano del telefono"
        );
        assert_eq!(ordine.len(), 4);
        assert_eq!(
            telefono.fondi(&portatile).ordine(),
            ordine,
            "l'ordine non può dipendere da chi si è sincronizzato per primo"
        );
    }

    #[test]
    fn togliere_un_brano_non_se_lo_rimette_l_altro_dispositivo() {
        let base = Sequenza::dalla_lista("comune", &lista(&["a", "b", "c"]));
        let mut chi_toglie = base.clone();
        assert!(chi_toglie.togli(1, 500));
        assert_eq!(chi_toglie.ordine(), vec!["a", "c"]);
        // L'altro non ha visto niente e ha ancora tutti e tre.
        assert_eq!(
            chi_toglie.fondi(&base).ordine(),
            vec!["a", "c"],
            "la lapide deve vincere sull'ignoranza dell'altro"
        );
        assert_eq!(base.fondi(&chi_toglie).ordine(), vec!["a", "c"]);
    }

    #[test]
    fn rimettere_un_brano_tolto_e_un_elemento_nuovo() {
        let mut sequenza = Sequenza::dalla_lista("uno", &lista(&["a", "b"]));
        sequenza.togli(0, 100);
        sequenza.accoda("uno", "a");
        assert_eq!(sequenza.ordine(), vec!["b", "a"]);
        assert_eq!(sequenza.quanti(), 3, "la lapide resta");
    }

    #[test]
    fn lo_stesso_brano_puo_stare_due_volte() {
        // `playlist_tracks` lo ammette — la chiave è (playlist, posizione) — e una
        // struttura che identificasse gli elementi col brano lo vieterebbe.
        let sequenza = Sequenza::dalla_lista("uno", &lista(&["a", "b", "a"]));
        assert_eq!(sequenza.ordine(), vec!["a", "b", "a"]);
    }

    #[test]
    fn riordinare_sposta_uno_e_non_riscrive_tutto() {
        let mut sequenza = Sequenza::dalla_lista("uno", &lista(&["a", "b", "c", "d"]));
        let prima = sequenza.quanti();
        sequenza.riordina("uno", &lista(&["a", "c", "b", "d"]), 100);
        assert_eq!(sequenza.ordine(), vec!["a", "c", "b", "d"]);
        assert!(
            sequenza.quanti() <= prima + 2,
            "spostare un brano non deve costare una lapide per ogni brano: \
             {prima} elementi prima, {} dopo",
            sequenza.quanti()
        );
    }

    #[test]
    fn riordinare_toglie_quel_che_non_c_e_piu_e_aggiunge_quel_che_e_nuovo() {
        let mut sequenza = Sequenza::dalla_lista("uno", &lista(&["a", "b", "c"]));
        sequenza.riordina("uno", &lista(&["c", "z", "a"]), 100);
        assert_eq!(sequenza.ordine(), vec!["c", "z", "a"]);
    }

    #[test]
    fn riordinare_e_poi_fondere_col_vecchio_non_resuscita_niente() {
        let base = Sequenza::dalla_lista("uno", &lista(&["a", "b", "c"]));
        let mut riordinata = base.clone();
        riordinata.riordina("uno", &lista(&["c", "a"]), 100);
        assert_eq!(riordinata.fondi(&base).ordine(), vec!["c", "a"]);
        assert_eq!(base.fondi(&riordinata).ordine(), vec!["c", "a"]);
    }

    #[test]
    fn inserire_in_testa_e_in_mezzo() {
        let mut sequenza = Sequenza::dalla_lista("uno", &lista(&["b", "d"]));
        sequenza.inserisci("uno", 0, "a");
        assert_eq!(sequenza.ordine(), vec!["a", "b", "d"]);
        sequenza.inserisci("uno", 2, "c");
        assert_eq!(sequenza.ordine(), vec!["a", "b", "c", "d"]);
        sequenza.inserisci("uno", 99, "e");
        assert_eq!(
            sequenza.ordine(),
            vec!["a", "b", "c", "d", "e"],
            "una posizione oltre la fine accoda"
        );
    }

    #[test]
    fn fondere_e_commutativo_associativo_e_idempotente() {
        let base = Sequenza::dalla_lista("comune", &lista(&["a", "b"]));
        let mut uno = base.clone();
        uno.accoda("uno", "x");
        let mut due = base.clone();
        due.accoda("due", "y");
        let mut tre = base.clone();
        tre.togli(0, 50);

        assert_eq!(uno.fondi(&due).ordine(), due.fondi(&uno).ordine());
        assert_eq!(uno.fondi(&uno), uno);
        assert_eq!(
            uno.fondi(&due).fondi(&tre).ordine(),
            uno.fondi(&due.fondi(&tre)).ordine()
        );
    }

    #[test]
    fn un_orfano_non_si_perde() {
        // L'elemento punta a un ancoraggio che non abbiamo: succede quando il
        // documento del dispositivo che lo ha scritto non è ancora arrivato.
        let mut sequenza = Sequenza::dalla_lista("uno", &lista(&["a"]));
        let mut orfano = Sequenza::vuota();
        orfano.elementi.insert(
            Identita::nuova("altro", 9),
            Elemento {
                dopo: Some(Identita::nuova("mai-visto", 3)),
                brano: "smarrito".to_owned(),
                tolto: None,
            },
        );
        sequenza = sequenza.fondi(&orfano);
        assert!(
            sequenza.ordine().contains(&"smarrito"),
            "un orfano si accoda, non si butta"
        );
    }

    #[test]
    fn un_ciclo_non_manda_in_ciclo_la_visita() {
        // Non può prodursi da solo, ma il documento arriva da fuori e «l'abbiamo
        // scritto noi» non è una garanzia su cosa ci sia dentro adesso.
        let primo = Identita::nuova("uno", 1);
        let secondo = Identita::nuova("uno", 2);
        let mut sequenza = Sequenza::vuota();
        sequenza.elementi.insert(
            primo.clone(),
            Elemento {
                dopo: Some(secondo.clone()),
                brano: "a".to_owned(),
                tolto: None,
            },
        );
        sequenza.elementi.insert(
            secondo,
            Elemento {
                dopo: Some(primo),
                brano: "b".to_owned(),
                tolto: None,
            },
        );
        let ordine = sequenza.ordine();
        assert_eq!(ordine.len(), 2, "nessuno dei due si perde");
    }

    #[test]
    fn l_identita_fa_il_giro() {
        let identita = Identita::nuova("aBc-123_", 42);
        assert_eq!(identita.testo(), "aBc-123_:42");
        assert_eq!(Identita::da_testo(&identita.testo()), Some(identita));
        assert_eq!(Identita::da_testo("senza-numero"), None);
        assert_eq!(Identita::da_testo(":7"), None);
        assert_eq!(Identita::da_testo("uno:non-un-numero"), None);
    }

    #[test]
    fn l_ordine_delle_identita_e_per_numero_e_poi_per_dispositivo() {
        // Ordinare prima per dispositivo vorrebbe dire che tutto quel che fa un
        // computer chiamato «aaa» viene sempre prima di tutto quel che fa «zzz».
        assert!(Identita::nuova("zzz", 1) < Identita::nuova("aaa", 2));
        assert!(Identita::nuova("aaa", 1) < Identita::nuova("zzz", 1));
    }

    #[test]
    fn una_sequenza_fa_il_giro_in_json() {
        let mut sequenza = Sequenza::dalla_lista("uno", &lista(&["a", "b"]));
        sequenza.togli(0, 100);
        sequenza.accoda("due", "c");
        let testo = serde_json::to_string(&sequenza).expect("serializza");
        let riletta: Sequenza = serde_json::from_str(&testo).expect("interpreta");
        assert_eq!(riletta, sequenza);
        assert_eq!(riletta.ordine(), sequenza.ordine());
        assert_eq!(
            serde_json::to_string(&riletta).expect("riserializza"),
            testo,
            "il giro deve essere stabile byte per byte, o l'impronta cambia da sola"
        );
    }
}
