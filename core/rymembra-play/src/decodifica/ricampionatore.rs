//! Da una frequenza all'altra, senza perdere né aggiungere un fotogramma.
//!
//! # I due buchi che c'erano
//!
//! Il ricampionatore di rubato non è istantaneo: è un filtro, e un filtro ha
//! un **ritardo**. Fra 44 100 e 48 000 `FftFixedIn` lavora a pezzi di 588
//! fotogrammi in ingresso e 640 in uscita, e ogni campione esce 320 fotogrammi
//! d'uscita dopo il momento a cui appartiene — `output_delay()`. In più
//! trattiene fino a 587 fotogrammi d'ingresso finché non ha un pezzo intero da
//! trasformare.
//!
//! Prima non si teneva conto di nessuna delle due cose, e ai confini di ogni
//! brano si pagava due volte:
//!
//! - **in coda**, una sola chiamata «parziale» con gli ultimi campioni
//!   imbottiti di zeri: quel che era ancora dentro il filtro, e quel che stava
//!   ancora aspettando il pezzo intero, non usciva mai. Una ventina di
//!   millisecondi persi alla fine di ogni brano;
//! - **in testa**, i 320 fotogrammi di ritardo uscivano come silenzio prima
//!   della musica: sei millisecondi e mezzo di vuoto all'inizio di ogni brano.
//!
//! Su un disco dal vivo o su un missaggio senza stacchi quei ventisei
//! millisecondi sono uno schiocco a ogni cambio di traccia, e su Windows il
//! dispositivo apre quasi sempre a 48 000 mentre i dischi sono a 44 100: cioè
//! capitava a quasi tutti, quasi sempre.
//!
//! # Come si chiude
//!
//! Con due conti. Si ricorda quanti fotogrammi sono **entrati** da quando il
//! ricampionatore è stato azzerato, e quanti ne sono **usciti**:
//!
//! - i primi `output_delay()` fotogrammi prodotti si buttano, così il primo che
//!   esce è il primo del brano e non il ritardo del filtro;
//! - a fine brano si continua a spingere silenzio finché non è uscito tutto
//!   quel che spetta, cioè `ceil(entrati × uscita / ingresso)` fotogrammi — e
//!   lì si taglia, così né più né meno.
//!
//! Un salto riporta tutto a zero: la memoria del filtro appartiene al punto da
//! cui si è saltati via, e tenerla vorrebbe dire far uscire una dozzina di
//! millisecondi del passato dopo il salto.
//!
//! # La giuntura fra due brani
//!
//! Due brani con due ricampionatori non fanno un segnale continuo, per due
//! ragioni. Ciascuno vede zeri oltre il proprio bordo: gli ultimi campioni del
//! primo sono calcolati come se dopo ci fosse silenzio, i primi del secondo
//! come se ci fosse silenzio prima, e intorno alla giuntura resta
//! l'increspatura di Gibbs dei due bordi. E il secondo ricomincia la griglia
//! d'uscita da capo: il suo primo fotogramma cade sul suo primo campione,
//! mentre sulla griglia continua cadrebbe una frazione di campione più in là
//! — `entrati × uscita / ingresso` non è quasi mai intero. Su un seno a 1 kHz
//! quello spostamento è un gradino di un decimo dell'ampiezza, e si sente:
//! il collaudo dal vivo della 3.0 l'ha registrato come uno schiocco a ogni
//! giuntura di un album senza stacchi.
//!
//! `due_brani_di_fila_non_lasciano_buchi` misura il caso dei due
//! ricampionatori, che resta per i brani di forma diversa. Fra due brani della
//! stessa forma il ricampionatore invece **passa** dal primo al secondo — vedi
//! [`super::Decodificatore::passa_la_coda`] — e la giuntura la attraversa un
//! filtro solo, come se il disco fosse un file unico:
//! `un_ricampionatore_solo_attraversa_la_giuntura` lo verifica al campione.

use std::collections::VecDeque;

use rubato::Resampler as _;
use rymembra_domain::errors::{AppError, ErrorCode};

use super::canali::Matrice;
use super::{
    BLOCCO, CANALI_MAX, FREQUENZA_MAX, FREQUENZA_MIN, USCITA_MAX, frequenza_accettabile,
    frequenza_d_uscita_accettabile,
};

/// Il ricampionatore, con le sue code per canale e i suoi due conti.
///
/// Rubato lavora a blocchi di dimensione fissa e su dati **per canale**, mentre
/// il decodificatore consegna blocchi di lunghezza variabile e interlacciati.
/// Questa struttura è il traduttore fra le due forme: accumula, converte quando
/// ha abbastanza roba, e conserva il resto per la volta dopo.
pub(super) struct Ricampionatore {
    interno: rubato::FftFixedIn<f32>,
    frequenza_ingresso: u32,
    frequenza_uscita: u32,
    /// Una coda per canale, in attesa di fare un blocco intero.
    code: Vec<VecDeque<f32>>,
    /// Buffer riusati, per non allocare a ogni blocco.
    ingresso: Vec<Vec<f32>>,
    uscita: Vec<Vec<f32>>,
    /// Un fotogramma d'uscita per canale della sorgente, prima della matrice.
    fotogramma: Vec<f32>,
    /// Fotogrammi della sorgente entrati dall'ultimo azzeramento.
    entrati: u64,
    /// Fotogrammi consegnati dall'ultimo azzeramento, ritardo escluso.
    usciti: u64,
    /// Quanti fotogrammi d'uscita restano da buttare perché sono il ritardo.
    ritardo_da_buttare: usize,
    /// Quanti blocchi di silenzio al massimo per svuotare il filtro a fine
    /// brano. Vedi [`Ricampionatore::coda`].
    giri_massimi: u64,
    track_id: i64,
}

impl Ricampionatore {
    /// Costruisce il traduttore fra le due frequenze, se le due frequenze
    /// hanno senso.
    ///
    /// # Perché ricontrolla quel che l'apertura ha già controllato
    ///
    /// Perché il rifiuto di là guarda la frequenza del **file** e questo è il
    /// solo punto che veda anche quella dell'**uscita**, che arriva dal
    /// dispositivo e non dal file. E perché `FftFixedIn::new` alloca in
    /// funzione del rapporto fra le due: chi lo chiama deve poter contare sul
    /// fatto che i due numeri siano già stati guardati, senza doverselo
    /// ricordare.
    ///
    /// Ognuna col **suo** tetto. Quello dei file, 384 kHz, misurato
    /// sull'uscita rifiutava ogni brano su un DAC impostato a 705,6 o 768 kHz,
    /// e lo rifiutava come file danneggiato: l'uscita ha il suo, vedi
    /// `decodifica::USCITA_MAX`. Un'uscita oltre quello non arriva fin qui —
    /// la rifiuta l'apertura del dispositivo — e il controllo resta per chi
    /// costruisce un ricampionatore con un numero che non viene da lì.
    ///
    /// # Perché nessun ripiego
    ///
    /// Qui c'era `usize::try_from(da).unwrap_or(44_100)`: una conversione che
    /// non può fallire, con accanto un numero **inventato** per il caso in cui
    /// fallisse. Se mai fosse scattato, il ricampionatore avrebbe convertito da
    /// una frequenza che il file non ha — cioè avrebbe suonato il brano alla
    /// velocità sbagliata, in silenzio e per sempre, senza che niente da
    /// nessuna parte lo dicesse. Un errore restituito è la sola risposta onesta
    /// a un numero che non si sa convertire.
    ///
    /// # Errori
    ///
    /// `playback.decodeFailed` se una delle due frequenze è fuori scala, se i
    /// canali sono zero o troppi, o se rubato rifiuta la coppia.
    pub(super) fn nuovo(da: u32, a: u32, canali: usize, track_id: i64) -> Result<Self, AppError> {
        let rifiuta = |causa: String| {
            AppError::new(ErrorCode::PlaybackDecodeFailed {
                track_id: Some(track_id),
                format: None,
            })
            .with_cause(causa)
        };
        if !frequenza_accettabile(da) {
            return Err(rifiuta(format!(
                "ricampionatore {da}→{a}: frequenza del file fuori scala, attesa fra \
                 {FREQUENZA_MIN} e {FREQUENZA_MAX}"
            )));
        }
        if !frequenza_d_uscita_accettabile(a) {
            return Err(rifiuta(format!(
                "ricampionatore {da}→{a}: frequenza d'uscita fuori scala, attesa fra \
                 {FREQUENZA_MIN} e {USCITA_MAX}"
            )));
        }
        if canali == 0 || canali > CANALI_MAX {
            return Err(rifiuta(format!(
                "ricampionatore a {canali} canali: attesi fra 1 e {CANALI_MAX}"
            )));
        }
        let (Ok(ingresso_hz), Ok(uscita_hz)) = (usize::try_from(da), usize::try_from(a)) else {
            return Err(rifiuta(format!(
                "ricampionatore {da}→{a}: le frequenze non entrano in un conteggio"
            )));
        };
        let interno = rubato::FftFixedIn::<f32>::new(ingresso_hz, uscita_hz, BLOCCO, 2, canali)
            .map_err(|err| rifiuta(format!("ricampionatore {da}→{a}: {err}")))?;
        let uscita = interno.output_buffer_allocate(true);
        let ritardo = interno.output_delay();
        let giri_massimi = giri_per_svuotare(ritardo, da, a);
        Ok(Self {
            interno,
            frequenza_ingresso: da,
            frequenza_uscita: a,
            code: vec![VecDeque::new(); canali],
            ingresso: vec![vec![0.0; BLOCCO]; canali],
            uscita,
            fotogramma: vec![0.0; canali],
            entrati: 0,
            usciti: 0,
            ritardo_da_buttare: ritardo,
            giri_massimi,
            track_id,
        })
    }

    /// La frequenza dei campioni che si aspetta.
    pub(super) const fn frequenza_ingresso(&self) -> u32 {
        self.frequenza_ingresso
    }

    /// Quanti canali si aspetta.
    pub(super) fn canali(&self) -> usize {
        self.code.len()
    }

    /// Non ha ancora visto un fotogramma dall'ultimo azzeramento.
    pub(super) const fn intatto(&self) -> bool {
        self.entrati == 0 && self.usciti == 0
    }

    /// Da qui in poi lavora per un altro brano: è il nome che mette nei
    /// propri errori. Vedi [`super::Decodificatore::passa_la_coda`].
    pub(super) const fn passa_a(&mut self, track_id: i64) {
        self.track_id = track_id;
    }

    /// Dimentica tutto: code, memoria del filtro e i due conti.
    ///
    /// Dopo un salto. `reset()` di rubato azzera la sovrapposizione fra un
    /// pezzo e l'altro e i fotogrammi trattenuti: senza, la dozzina di
    /// millisecondi che il filtro ha ancora in pancia uscirebbe dopo il salto,
    /// cioè un frammento del punto da cui si è saltati via.
    pub(super) fn svuota(&mut self) {
        for c in &mut self.code {
            c.clear();
        }
        self.interno.reset();
        self.entrati = 0;
        self.usciti = 0;
        self.ritardo_da_buttare = self.interno.output_delay();
    }

    /// Quanti fotogrammi d'uscita spettano a quel che è entrato finora.
    ///
    /// `ceil(entrati × uscita / ingresso)`: il fotogramma d'uscita `k` sta
    /// all'istante `k / uscita`, e l'ultimo che appartiene al brano è l'ultimo
    /// che cade prima della fine dell'ultimo campione entrato.
    fn attesi(&self) -> u64 {
        let numeratore = u128::from(self.entrati) * u128::from(self.frequenza_uscita);
        let denominatore = u128::from(self.frequenza_ingresso);
        if denominatore == 0 {
            return 0;
        }
        u64::try_from(numeratore.div_ceil(denominatore)).unwrap_or(u64::MAX)
    }

    /// Quanti fotogrammi d'uscita sono ancora dentro il filtro.
    pub(super) fn in_sospeso(&self) -> u64 {
        self.attesi().saturating_sub(self.usciti)
    }

    /// Accumula un blocco interlacciato e consegna quel che ne esce.
    pub(super) fn spingi(
        &mut self,
        campioni: &[f32],
        matrice: &Matrice,
        fuori: &mut Vec<f32>,
    ) -> Result<(), AppError> {
        let canali = self.code.len();
        if canali == 0 {
            return Ok(());
        }
        let mut quanti = 0_u64;
        for blocco in campioni.chunks_exact(canali) {
            for (coda, campione) in self.code.iter_mut().zip(blocco) {
                coda.push_back(*campione);
            }
            quanti = quanti.saturating_add(1);
        }
        self.entrati = self.entrati.saturating_add(quanti);
        while self.code.first().is_some_and(|c| c.len() >= BLOCCO) {
            self.riempi();
            self.converti(None, matrice, fuori)?;
        }
        Ok(())
    }

    /// Consegna quel che resta alla fine del brano, fino all'ultimo
    /// fotogramma che gli spetta e non uno di più.
    ///
    /// Si spinge silenzio finché il filtro non ha restituito tutto quel che
    /// aveva — il resto delle code, i fotogrammi che aspettavano un pezzo
    /// intero, il ritardo — e l'ultimo giro si tronca a [`Self::attesi`].
    ///
    /// [`Self::giri_massimi`] non serve a un file normale, che finisce in due
    /// giri: è la garanzia che nessuna coppia di frequenze strana — dove un
    /// pezzo di `FftFixedIn` può essere di decine di migliaia di fotogrammi —
    /// possa trasformare la fine di un brano in un ciclo senza fine.
    pub(super) fn coda(&mut self, matrice: &Matrice, fuori: &mut Vec<f32>) -> Result<(), AppError> {
        let attesi = self.attesi();
        let mut giri = 0_u64;
        while self.usciti < attesi && giri < self.giri_massimi {
            self.riempi();
            self.converti(Some(attesi.saturating_sub(self.usciti)), matrice, fuori)?;
            giri = giri.saturating_add(1);
        }
        for c in &mut self.code {
            c.clear();
        }
        Ok(())
    }

    /// Un blocco intero dalle code ai buffer d'ingresso, con silenzio dove le
    /// code non arrivano.
    fn riempi(&mut self) {
        for (coda, piano) in self.code.iter_mut().zip(self.ingresso.iter_mut()) {
            for posto in piano.iter_mut() {
                *posto = coda.pop_front().unwrap_or(0.0);
            }
        }
    }

    /// Un giro di rubato, e quel che ne esce passato per la matrice.
    ///
    /// Sempre `process_into_buffer` e mai la variante «parziale»: quella
    /// imbottisce di zeri in un buffer che alloca a ogni chiamata, e qui
    /// l'imbottitura la fa già [`Self::riempi`] nei buffer riusati.
    fn converti(
        &mut self,
        limite: Option<u64>,
        matrice: &Matrice,
        fuori: &mut Vec<f32>,
    ) -> Result<(), AppError> {
        let (_, prodotti) = self
            .interno
            .process_into_buffer(&self.ingresso, &mut self.uscita, None)
            .map_err(|err| {
                AppError::new(ErrorCode::PlaybackDecodeFailed {
                    track_id: Some(self.track_id),
                    format: None,
                })
                .with_cause(err.to_string())
            })?;
        // Il ritardo del filtro, in testa: vedi l'intestazione del modulo.
        let buttati = self.ritardo_da_buttare.min(prodotti);
        self.ritardo_da_buttare = self.ritardo_da_buttare.saturating_sub(buttati);
        let mut validi = prodotti.saturating_sub(buttati);
        if let Some(limite) = limite {
            validi = validi.min(usize::try_from(limite).unwrap_or(usize::MAX));
        }
        // Da per-canale a interlacciato, passando per la matrice dei canali.
        for indice in buttati..buttati.saturating_add(validi) {
            for (posto, piano) in self.fotogramma.iter_mut().zip(&self.uscita) {
                *posto = piano.get(indice).copied().unwrap_or(0.0);
            }
            matrice.applica_fotogramma(&self.fotogramma, fuori);
        }
        self.usciti = self
            .usciti
            .saturating_add(u64::try_from(validi).unwrap_or(u64::MAX));
        Ok(())
    }
}

/// Quanti blocchi di silenzio bastano sempre a svuotare il filtro.
///
/// Il caso peggiore: le code tengono meno di un [`BLOCCO`], rubato trattiene
/// meno di un pezzo intero, e il ritardo vale mezzo pezzo. Il pezzo
/// d'ingresso non si legge da rubato, ma si ricava dal ritardo — che è metà del
/// pezzo d'uscita — riportato alla frequenza d'ingresso. Due pezzi e un blocco,
/// arrotondati per eccesso, più due giri di margine.
fn giri_per_svuotare(ritardo: usize, da: u32, a: u32) -> u64 {
    let pezzo_uscita = u128::try_from(ritardo)
        .unwrap_or(u128::MAX)
        .saturating_mul(2);
    let pezzo_ingresso = pezzo_uscita
        .saturating_mul(u128::from(da))
        .div_ceil(u128::from(a.max(1)));
    let blocco = u128::try_from(BLOCCO).unwrap_or(1).max(1);
    let fotogrammi = pezzo_ingresso.saturating_mul(2).saturating_add(blocco);
    u64::try_from(fotogrammi.div_ceil(blocco))
        .unwrap_or(u64::MAX)
        .saturating_add(2)
}

#[cfg(test)]
mod prove {
    use symphonia::core::audio::Channels;

    use super::*;

    /// Un coseno a `hz`, lungo `quanti` fotogrammi a `frequenza`, a partire dal
    /// fotogramma `da` — così due pezzi consecutivi sono lo stesso segnale.
    ///
    /// Coseno e non seno apposta: comincia dal massimo, e in
    /// [`due_brani_di_fila_non_lasciano_buchi`] cade al massimo anche sulla
    /// giuntura. Un seno ci arriverebbe passando per lo zero, dove il bordo di
    /// un filtro non si vede — ed è il caso che la prova deve guardare, non
    /// quello che la fa passare.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "un seno a mezza ampiezza sta in un f32 senza perdere niente che conti"
    )]
    fn seno(hz: f64, frequenza: u32, da: usize, quanti: usize) -> Vec<f32> {
        (da..da + quanti)
            .map(|n| {
                let t = n as f64 / f64::from(frequenza);
                (2.0 * std::f64::consts::PI * hz * t).cos() as f32 * 0.5
            })
            .collect()
    }

    fn mono() -> Matrice {
        Matrice::nuova(&Channels::None, 1, 1)
    }

    /// Tutto un brano mono attraverso un ricampionatore nuovo, a blocchi
    /// irregolari come quelli che consegna un decodificatore.
    fn ricampiona(ingresso: &[f32], da: u32, a: u32) -> Vec<f32> {
        let mut r = Ricampionatore::nuovo(da, a, 1, 1).expect("costruito");
        let m = mono();
        let mut fuori = Vec::new();
        for pezzo in ingresso.chunks(1_152) {
            r.spingi(pezzo, &m, &mut fuori).expect("spinto");
        }
        r.coda(&m, &mut fuori).expect("coda");
        fuori
    }

    fn attesi(quanti: usize, da: u32, a: u32) -> usize {
        let quanti = u64::try_from(quanti).expect("sta in un u64");
        usize::try_from((quanti * u64::from(a)).div_ceil(u64::from(da))).expect("sta in un usize")
    }

    #[test]
    fn una_frequenza_assurda_non_arriva_al_ricampionatore() {
        // Tre megahertz è il numero che esce da un'intestazione troncata, ed è
        // il caso caro: fra tre milioni e quarantottomila il massimo comun
        // divisore è piccolo, e `FftFixedIn::new` dimensiona i propri buffer su
        // quel rapporto. Prima costruiva il mostro e poi ci lavorava dentro.
        assert!(Ricampionatore::nuovo(3_000_000, 48_000, 2, 1).is_err());
        // Zero da una parte e dall'altra: la frequenza dell'uscita arriva dal
        // dispositivo, non dal file, ed è l'unico posto che la guardi.
        assert!(Ricampionatore::nuovo(0, 48_000, 2, 1).is_err());
        assert!(Ricampionatore::nuovo(44_100, 0, 2, 1).is_err());
        // E il caso normale continua a costruirsi: è tutta la libreria di
        // questo progetto.
        assert!(Ricampionatore::nuovo(44_100, 48_000, 2, 1).is_ok());
    }

    #[test]
    fn un_uscita_a_768_khz_ricampiona_i_file_a_44_1() {
        // Un DAC USB impostato in Windows a 705,6 o 768 kHz: il formato del mix
        // lo dice il dispositivo, e nessun file ha quella frequenza. Misurata
        // col tetto dei **file**, l'uscita faceva fallire ogni brano come
        // «file danneggiato».
        assert!(Ricampionatore::nuovo(44_100, 768_000, 2, 1).is_ok());
        assert!(Ricampionatore::nuovo(44_100, 705_600, 2, 1).is_ok());
        let ingresso = vec![0.25f32; 44_100];
        assert_eq!(
            ricampiona(&ingresso, 44_100, 768_000).len(),
            attesi(44_100, 44_100, 768_000)
        );
        // Il tetto dei file resta per i file: un ingresso a 768 kHz è ancora
        // un'intestazione letta male.
        assert!(Ricampionatore::nuovo(768_000, 48_000, 2, 1).is_err());
        // E l'uscita ha il suo: oltre, è un dispositivo che non sappiamo servire.
        assert!(Ricampionatore::nuovo(44_100, 768_001, 2, 1).is_err());
    }

    #[test]
    fn nessun_canale_e_troppi_canali_non_costruiscono_niente() {
        assert!(Ricampionatore::nuovo(44_100, 48_000, 0, 1).is_err());
        assert!(Ricampionatore::nuovo(44_100, 48_000, CANALI_MAX + 1, 1).is_err());
        // Il tetto sta sopra qualunque configurazione reale: il 22.2 giapponese
        // ne conta ventiquattro, e deve passare.
        assert!(Ricampionatore::nuovo(44_100, 48_000, 24, 1).is_ok());
    }

    #[test]
    fn la_lunghezza_in_uscita_e_esatta() {
        // `ceil(in × rapporto)`, al fotogramma, per lunghezze qualunque —
        // comprese quelle che non riempiono nemmeno un blocco e quelle che
        // cadono a un fotogramma da un confine.
        for (da, a) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (22_050, 48_000),
            (96_000, 48_000),
        ] {
            for quanti in [1, 7, 587, 588, 1_023, 1_024, 1_025, 12_345, 44_100, 44_101] {
                let fuori = ricampiona(&vec![0.25; quanti], da, a);
                assert_eq!(
                    fuori.len(),
                    attesi(quanti, da, a),
                    "{quanti} fotogrammi da {da} a {a}"
                );
            }
        }
    }

    #[test]
    fn niente_ritardo_in_testa() {
        // Un coseno a 1 kHz portato da 44,1 a 48: fotogramma per fotogramma
        // deve coincidere con lo stesso coseno calcolato direttamente a 48.
        // Uno scarto anche di un solo fotogramma darebbe un errore fino a
        // 0,065 a quest'ampiezza; i 320 del ritardo non buttato darebbero un
        // segnale spostato di due terzi di periodo. Misurato: tre milionesimi.
        let ingresso = seno(1_000.0, 44_100, 0, 44_100);
        let fuori = ricampiona(&ingresso, 44_100, 48_000);
        let ideale = seno(1_000.0, 48_000, 0, 48_000);
        assert_eq!(fuori.len(), ideale.len());
        // Lontano dai bordi, dove il filtro vede musica da tutte e due le
        // parti.
        let peggiore = fuori[200..47_800]
            .iter()
            .zip(&ideale[200..47_800])
            .map(|(x, y)| (x - y).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            peggiore < 1e-3,
            "scarto massimo dal seno ideale: {peggiore}"
        );
        // E già i primi fotogrammi stanno sul coseno e non nel silenzio del
        // ritardo, che li avrebbe lasciati a zero dove il segnale vale 0,5.
        // Il decimo risente ancora del bordo, di un millesimo.
        assert!(
            (fuori[10] - ideale[10]).abs() < 0.01,
            "{} contro {}",
            fuori[10],
            ideale[10]
        );
    }

    #[test]
    fn due_brani_di_fila_non_lasciano_buchi() {
        // Un coseno spezzato in due brani, ciascuno col suo ricampionatore come
        // succede fra due tracce di un disco, con la giuntura sul massimo: la
        // lunghezza è esatta, e dopo la giuntura il segnale continua in fase —
        // niente fotogrammi persi né doppi. La sbavatura intorno alla giuntura
        // è quel che resta di due filtri che vedono zeri oltre il proprio
        // bordo: la si misura, e deve restare stretta.
        let primo = seno(440.0, 44_100, 0, 44_100);
        let secondo = seno(440.0, 44_100, 44_100, 22_050);
        let mut fuori = ricampiona(&primo, 44_100, 48_000);
        let giuntura = fuori.len();
        assert_eq!(giuntura, 48_000);
        fuori.extend(ricampiona(&secondo, 44_100, 48_000));
        assert_eq!(fuori.len(), 72_000);

        let ideale = seno(440.0, 48_000, 0, 72_000);
        let scarto = |da: usize, a: usize| {
            fuori[da..a]
                .iter()
                .zip(&ideale[da..a])
                .map(|(x, y)| (x - y).abs())
                .fold(0.0_f32, f32::max)
        };
        // Lontano dalla giuntura il segnale è quello ideale, da tutte e due le
        // parti: nessuno spostamento di fase. (Misurato: otto decimillesimi,
        // che sono la coda dell'increspatura del bordo iniziale.)
        assert!(scarto(200, giuntura - 64) < 2e-3);
        assert!(scarto(giuntura + 64, 71_800) < 2e-3);
        // A cavallo della giuntura: prima c'erano ventisei millisecondi di
        // silenzio, cioè uno scarto pari all'ampiezza intera per 1250
        // fotogrammi. Ora resta l'increspatura di Gibbs dei due bordi:
        // misurata 0,036 su un'ampiezza di 0,5, di segno alterno da un
        // fotogramma all'altro — cioè energia attorno ai 20–24 kHz, al limite
        // della banda — e sotto lo 0,006 già a otto fotogrammi di distanza.
        let vicino = scarto(giuntura - 64, giuntura + 64);
        assert!(vicino < 0.08, "sbavatura alla giuntura: {vicino}");
        let appena_fuori =
            scarto(giuntura - 64, giuntura - 8).max(scarto(giuntura + 8, giuntura + 64));
        assert!(
            appena_fuori < 0.015,
            "la sbavatura si allarga: {appena_fuori}"
        );
    }

    #[test]
    fn dopo_un_salto_non_resta_niente_del_prima() {
        // Musica a mezza ampiezza, poi il salto, poi silenzio: tutto quel che
        // esce dopo il salto deve essere silenzio. Prima dello `reset()` di
        // rubato ne uscivano una dozzina di millisecondi del punto di prima.
        let mut r = Ricampionatore::nuovo(44_100, 48_000, 1, 1).expect("costruito");
        let m = mono();
        let mut fuori = Vec::new();
        r.spingi(&seno(1_000.0, 44_100, 0, 10_000), &m, &mut fuori)
            .expect("spinto");
        assert!(!fuori.is_empty());

        r.svuota();
        fuori.clear();
        r.spingi(&vec![0.0; 5_000], &m, &mut fuori).expect("spinto");
        r.coda(&m, &mut fuori).expect("coda");
        assert_eq!(fuori.len(), attesi(5_000, 44_100, 48_000));
        let residuo = fuori.iter().fold(0.0_f32, |max, c| max.max(c.abs()));
        assert!(
            residuo < 1e-6,
            "dopo il salto esce ancora il passato: {residuo}"
        );
    }

    #[test]
    fn i_canali_passano_per_la_matrice() {
        // Stereo in ingresso, due canali distinti, e la matrice che li porta
        // su un 5.1: il ricampionatore non deve mescolarli né spostarli.
        let mut r = Ricampionatore::nuovo(44_100, 48_000, 2, 1).expect("costruito");
        let m = Matrice::nuova(&Channels::None, 2, 6);
        let mut dentro = Vec::with_capacity(8_820);
        for _ in 0..4_410 {
            dentro.push(0.5);
            dentro.push(-0.25);
        }
        let mut fuori = Vec::new();
        r.spingi(&dentro, &m, &mut fuori).expect("spinto");
        r.coda(&m, &mut fuori).expect("coda");
        assert_eq!(fuori.len(), attesi(4_410, 44_100, 48_000) * 6);
        let in_mezzo = &fuori[6 * 2_000..6 * 2_001];
        assert!((in_mezzo[0] - 0.5).abs() < 1e-3, "{in_mezzo:?}");
        assert!((in_mezzo[1] + 0.25).abs() < 1e-3, "{in_mezzo:?}");
        assert!(in_mezzo[2..].iter().all(|c| c.abs() < 1e-6), "{in_mezzo:?}");
    }

    #[test]
    fn i_giri_per_svuotare_bastano_anche_per_le_frequenze_strane() {
        // 44 101 e 48 000 non hanno divisori comuni: il pezzo di rubato è di
        // 44 101 fotogrammi, e il filtro trattiene quasi un secondo. La coda
        // deve comunque arrivare alla lunghezza esatta.
        let fuori = ricampiona(&vec![0.1; 30_000], 44_101, 48_000);
        assert_eq!(fuori.len(), attesi(30_000, 44_101, 48_000));
    }
}
