//! L'identità di un PC: la sua chiave statica X25519.
//!
//! # Dove vive
//!
//! La metà segreta nel portachiavi di Windows, sotto `insieme.chiave.<id del
//! dispositivo>`: la legge e la scrive il desktop, questo crate la riceve come
//! testo ([`Identita::da_testo`]) e la restituisce come testo
//! ([`Identita::in_testo`]). L'id del dispositivo nel nome della voce serve a
//! due Rymembra su due cartelle dati dello stesso utente — il portachiavi è
//! uno solo — per non avere la stessa identità.
//!
//! La metà pubblica è quel che un altro PC ricorda di questo: la tabella
//! `insieme_pari` del desktop la tiene per esteso, e la scoperta ne fa
//! viaggiare solo l'[`Impronta`].

use rymembra_domain::errors::{AppError, ErrorCode};
use snow::params::DHChoice;
use snow::resolvers::{CryptoResolver as _, DefaultResolver};

/// La chiave pubblica di un PC: 32 byte.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChiavePubblica(pub [u8; 32]);

/// I primi 8 byte di blake3 della chiave pubblica.
///
/// Viaggia in chiaro nella risposta alla scoperta: chi cerca la confronta con
/// le chiavi dei PC associati per riconoscerli senza che il nome passi in
/// chiaro. Un PC che ospita è riconoscibile sulla rete; il suo nome no.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Impronta(pub [u8; 8]);

/// La chiave statica di questo PC, segreta compresa.
pub struct Identita {
    pub(crate) segreta: [u8; 32],
    pub(crate) pubblica: ChiavePubblica,
}

impl Identita {
    /// Una chiave nuova, dal generatore del sistema.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se il generatore non risponde.
    pub fn nuova() -> Result<Self, AppError> {
        // Trentadue byte qualunque sono una chiave segreta X25519 valida: il
        // «clamping» dei bit lo fa la moltiplicazione scalare, dentro
        // `curve25519-dalek`, ogni volta che la chiave si usa. Non serve
        // aggiustarli qui, e aggiustarli qui vorrebbe dire scrivere a mano un
        // pezzo di crittografia che la libreria fa già.
        let mut segreta = [0_u8; 32];
        getrandom::fill(&mut segreta).map_err(|err| {
            AppError::new(ErrorCode::InternalUnexpected { detail: None })
                .with_cause(format!("il generatore del sistema non risponde: {err}"))
        })?;
        Self::da_segreta(segreta)
    }

    /// La chiave salvata, dal testo di [`Identita::in_testo`].
    ///
    /// # Errori
    ///
    /// `insieme.identitaPersa` se il testo non è una chiave.
    pub fn da_testo(testo: &str) -> Result<Self, AppError> {
        // Né il testo né un suo pezzo nella causa: è la chiave segreta, e la
        // causa finisce nel diario.
        let segreta = crate::esadecimale::leggi::<32>(testo.trim()).ok_or_else(|| {
            AppError::new(ErrorCode::InsiemeIdentitaPersa)
                .with_cause("la voce del portachiavi non è una chiave di 32 byte")
        })?;
        Self::da_segreta(segreta).map_err(|err| {
            AppError::new(ErrorCode::InsiemeIdentitaPersa).with_cause(
                err.cause()
                    .unwrap_or("chiave pubblica non calcolabile")
                    .to_owned(),
            )
        })
    }

    /// La coppia intera dalla metà segreta: la pubblica la calcola il
    /// risolutore di `snow`, lo stesso che farà la stretta.
    ///
    /// Lo stesso e non un'altra libreria: la chiave pubblica che gli altri PC
    /// ricordano deve essere **quella** che la stretta Noise dimostrerà di
    /// possedere, e due implementazioni della stessa curva sono due occasioni
    /// di non essere d'accordo su un dettaglio.
    fn da_segreta(segreta: [u8; 32]) -> Result<Self, AppError> {
        let mut curva = DefaultResolver
            .resolve_dh(&DHChoice::Curve25519)
            .ok_or_else(|| {
                AppError::new(ErrorCode::InternalUnexpected { detail: None })
                    .with_cause("snow senza Curve25519")
            })?;
        if curva.priv_len() != segreta.len() || curva.pub_len() != 32 {
            return Err(
                AppError::new(ErrorCode::InternalUnexpected { detail: None })
                    .with_cause("snow con chiavi di un'altra misura"),
            );
        }
        curva.set(&segreta);
        let pubblica = <[u8; 32]>::try_from(curva.pubkey()).map_err(|_| {
            AppError::new(ErrorCode::InternalUnexpected { detail: None })
                .with_cause("snow con chiavi di un'altra misura")
        })?;
        Ok(Self {
            segreta,
            pubblica: ChiavePubblica(pubblica),
        })
    }

    /// La metà segreta, per la stretta.
    pub(crate) const fn segreta(&self) -> &[u8; 32] {
        &self.segreta
    }

    /// La chiave segreta come testo, per il portachiavi.
    #[must_use]
    pub fn in_testo(&self) -> String {
        crate::esadecimale::scrivi(&self.segreta)
    }

    /// La metà pubblica.
    #[must_use]
    pub const fn pubblica(&self) -> ChiavePubblica {
        self.pubblica
    }
}

/// La metà segreta non resta nella memoria del processo dopo che l'identità se
/// n'è andata — uscendo, o quando la sessione si ferma.
///
/// Non è una difesa da chi legge la memoria di un processo vivo: quello vede
/// anche la chiave in uso. È l'igiene di non lasciarla in pagine liberate, che
/// possono finire in un file di paging o in un dump di un arresto anomalo.
impl Drop for Identita {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.segreta);
    }
}

/// Mai la metà segreta, nemmeno in una stampa di prova.
impl std::fmt::Debug for Identita {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identita")
            .field("pubblica", &self.pubblica)
            .finish_non_exhaustive()
    }
}

impl ChiavePubblica {
    /// L'impronta: vedi [`Impronta`].
    #[must_use]
    pub fn impronta(&self) -> Impronta {
        let hash = blake3::hash(&self.0);
        let mut byte = [0_u8; 8];
        for (posto, b) in byte.iter_mut().zip(hash.as_bytes()) {
            *posto = *b;
        }
        Impronta(byte)
    }

    /// In esadecimale, per la tabella `insieme_pari`.
    #[must_use]
    pub fn in_testo(&self) -> String {
        crate::esadecimale::scrivi(&self.0)
    }

    /// Da esadecimale. `None` se non sono 32 byte.
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        crate::esadecimale::leggi::<32>(testo).map(Self)
    }
}

impl std::fmt::Debug for ChiavePubblica {
    /// Solo l'impronta: basta a distinguere due PC nel diario, e una chiave
    /// intera nel diario non serve a nessuno.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChiavePubblica({})", self.impronta())
    }
}

impl std::fmt::Display for Impronta {
    /// A gruppi di quattro cifre, come la mostra l'interfaccia: `3fa2 91c0 …`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let testo = crate::esadecimale::scrivi(&self.0);
        let mut primo = true;
        for gruppo in testo.as_bytes().chunks(4) {
            if !primo {
                f.write_str(" ")?;
            }
            primo = false;
            f.write_str(std::str::from_utf8(gruppo).unwrap_or(""))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_chiave_pubblica_e_quella_della_rfc_7748() {
        // Il vettore di Alice della RFC 7748, § 6.1: se il risolutore di snow
        // calcolasse la pubblica in un altro modo, gli altri PC ricorderebbero
        // una chiave che la stretta non dimostra mai.
        let alice =
            Identita::da_testo("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
                .expect("chiave valida");
        assert_eq!(
            alice.pubblica().in_testo(),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
    }

    #[test]
    fn una_chiave_nuova_torna_uguale_dal_portachiavi() {
        let nuova = Identita::nuova().expect("il generatore risponde");
        let riletta = Identita::da_testo(&nuova.in_testo()).expect("si rilegge");
        assert_eq!(riletta.pubblica(), nuova.pubblica());
        assert_ne!(
            nuova.pubblica(),
            Identita::nuova()
                .expect("il generatore risponde")
                .pubblica(),
            "due chiavi nuove sono due chiavi"
        );
    }

    #[test]
    fn un_testo_che_non_e_una_chiave_e_un_identita_persa() {
        for testo in ["", "abc", &"zz".repeat(32), &"ab".repeat(31)] {
            let err = Identita::da_testo(testo).expect_err("non è una chiave");
            assert_eq!(err.code().kind().code(), "insieme.identitaPersa");
        }
    }

    #[test]
    fn la_meta_segreta_non_si_stampa() {
        let identita = Identita::nuova().expect("il generatore risponde");
        let stampa = format!("{identita:?}");
        assert!(!stampa.contains(&identita.in_testo()), "{stampa}");
        assert!(
            !format!("{:?}", identita.pubblica()).contains(&identita.pubblica().in_testo()),
            "nemmeno la pubblica per esteso"
        );
    }

    #[test]
    fn l_impronta_si_scrive_a_gruppi_di_quattro() {
        let impronta = Impronta([0x3f, 0xa2, 0x91, 0xc0, 0, 1, 2, 3]);
        assert_eq!(impronta.to_string(), "3fa2 91c0 0001 0203");
    }
}
