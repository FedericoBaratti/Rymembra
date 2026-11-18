//! Il registro di Windows, per le due domande dell'avvio con Windows.
//!
//! # Perché sta qui
//!
//! Scrivere un valore in `HKCU\…\Run` e leggere `StartupApproved\Run` sono
//! chiamate `unsafe` del crate `windows` (`RegGetValueW`, `RegSetKeyValueW`,
//! `RegDeleteKeyValueW`), e la radice del workspace vieta `unsafe` con `forbid`:
//! stanno qui, nell'unico crate con l'eccezione dichiarata, dietro l'interfaccia
//! [`Registro`] che `unsafe` non lo chiede a nessuno. La regola — che cosa si
//! scrive, quando l'avvio vale acceso — sta in [`crate::avvio`], che parla con
//! quell'interfaccia e non col registro: è lì che le prove mettono un registro
//! finto, e **nessuna prova di Rymembra tocca il registro vero**.
//!
//! # Solo `HKEY_CURRENT_USER`
//!
//! L'installer NSIS è «per l'utente corrente» (nessun amministratore) e la
//! chiave `Run` che conta è la sua. Una chiave di macchina vorrebbe i permessi
//! che Rymembra non ha e non chiede.
//!
//! # I tre numeri che non sono un errore
//!
//! `ERROR_FILE_NOT_FOUND` e `ERROR_PATH_NOT_FOUND` dicono «il valore non c'è»,
//! e per una lettura è la risposta, non un guasto; per una cancellazione è già
//! fatta. `ERROR_UNSUPPORTED_TYPE` dice «c'è, ma non è di questo tipo»: per noi è
//! come se non fosse nostro.

/// Le quattro cose che l'avvio chiede al registro, sempre sotto
/// `HKEY_CURRENT_USER` e con il percorso della chiave scritto per esteso.
///
/// Un'interfaccia e non quattro funzioni libere per una ragione sola: le prove
/// dell'avvio non possono toccare il registro vero — quello di chi sta provando,
/// con la sua voce `Run` e il suo avvio — e con un'interfaccia mettono un
/// registro in memoria.
pub trait Registro {
    /// Il testo di un valore `REG_SZ`, o `None` se il valore non c'è (o non è
    /// un testo).
    ///
    /// # Errors
    ///
    /// Una frase che dice cosa ha risposto il sistema, se non è «non c'è».
    fn leggi_testo(&self, percorso: &str, nome: &str) -> Result<Option<String>, String>;

    /// I byte di un valore `REG_BINARY`, o `None` se il valore non c'è (o non è
    /// binario).
    ///
    /// # Errors
    ///
    /// Come [`Registro::leggi_testo`].
    fn leggi_byte(&self, percorso: &str, nome: &str) -> Result<Option<Vec<u8>>, String>;

    /// Scrive un valore `REG_SZ`, creando la chiave se manca.
    ///
    /// # Errors
    ///
    /// Una frase che dice cosa ha risposto il sistema.
    fn scrivi_testo(&self, percorso: &str, nome: &str, valore: &str) -> Result<(), String>;

    /// Toglie un valore. Un valore che non c'è è già tolto.
    ///
    /// # Errors
    ///
    /// Una frase che dice cosa ha risposto il sistema, se non è «non c'è».
    fn cancella(&self, percorso: &str, nome: &str) -> Result<(), String>;
}

/// Un testo come lo vuole il registro: UTF-16 little endian, col terminatore.
///
/// Pura e fuori dal blocco `unsafe`, con una prova: è la riga che fa leggere al
/// sistema un valore della lunghezza giusta.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "la usa solo il registro di Windows")
)]
fn a_utf16_con_zero(testo: &str) -> Vec<u8> {
    testo
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// L'inverso: i byte di un `REG_SZ` in un testo, fermandosi al primo
/// terminatore. Un numero dispari di byte lascia fuori l'ultimo.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "la usa solo il registro di Windows")
)]
fn da_utf16_con_zero(byte: &[u8]) -> String {
    let unita: Vec<u16> = byte
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .take_while(|unita| *unita != 0)
        .collect();
    String::from_utf16_lossy(&unita)
}

/// Il registro vero.
///
/// Si costruisce solo dove c'è: i valori che tocca sono quelli dell'utente che
/// sta usando il programma.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RegistroDiWindows;

#[cfg(windows)]
mod di_windows {
    use windows::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
        ERROR_UNSUPPORTED_TYPE, WIN32_ERROR,
    };
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_ROUTINE_FLAGS, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::core::HSTRING;

    use super::{Registro, RegistroDiWindows, a_utf16_con_zero, da_utf16_con_zero};

    /// Quante volte si rilegge se, fra la domanda della misura e quella del
    /// valore, qualcuno l'ha allungato.
    const TENTATIVI: usize = 3;

    /// Il valore grezzo, o `None` se non c'è o non è del tipo chiesto.
    #[allow(
        unsafe_code,
        reason = "RegGetValueW non ha un'interfaccia sicura: ogni blocco dice perché \
                  è corretto"
    )]
    fn leggi_grezzo(
        percorso: &str,
        nome: &str,
        tipo: REG_ROUTINE_FLAGS,
    ) -> Result<Option<Vec<u8>>, String> {
        let percorso = HSTRING::from(percorso);
        let nome = HSTRING::from(nome);
        for _ in 0..TENTATIVI {
            let mut misura: u32 = 0;
            // SAFETY: due `HSTRING` nostre, vive per tutta la chiamata e
            // terminate da zero; nessun buffer (`None`) e un solo puntatore, a
            // una variabile locale, per la misura. Windows ci scrive quanti
            // byte servono e non conserva il puntatore.
            let primo = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    &percorso,
                    &nome,
                    tipo,
                    None,
                    None,
                    Some(&raw mut misura),
                )
            };
            if e_assente(primo) {
                return Ok(None);
            }
            if primo != ERROR_SUCCESS {
                return Err(errore("la lettura", primo));
            }
            let Ok(lunghezza) = usize::try_from(misura) else {
                return Err("il valore è troppo grande".to_owned());
            };
            let mut dati = vec![0u8; lunghezza];
            let mut capienza = misura;
            // SAFETY: `dati` è un buffer nostro di `capienza` byte, vivo per
            // tutta la chiamata, e `capienza` è la misura che Windows gli
            // scrive accanto: non scrive oltre. Le due `HSTRING` come sopra.
            let secondo = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    &percorso,
                    &nome,
                    tipo,
                    None,
                    Some(dati.as_mut_ptr().cast()),
                    Some(&raw mut capienza),
                )
            };
            if e_assente(secondo) {
                return Ok(None);
            }
            if secondo == ERROR_MORE_DATA {
                // Qualcuno l'ha allungato fra le due domande: si rifà.
                continue;
            }
            if secondo != ERROR_SUCCESS {
                return Err(errore("la lettura", secondo));
            }
            dati.truncate(usize::try_from(capienza).unwrap_or(lunghezza));
            return Ok(Some(dati));
        }
        Err("il valore cambia mentre lo si legge".to_owned())
    }

    /// «Il valore non c'è», o «c'è ma di un altro tipo»: per noi, la stessa cosa.
    fn e_assente(esito: WIN32_ERROR) -> bool {
        esito == ERROR_FILE_NOT_FOUND
            || esito == ERROR_PATH_NOT_FOUND
            || esito == ERROR_UNSUPPORTED_TYPE
    }

    /// Una frase per un codice di Windows: quale operazione, e il numero.
    fn errore(cosa: &str, codice: WIN32_ERROR) -> String {
        format!("{cosa} del registro non riesce (codice {})", codice.0)
    }

    impl Registro for RegistroDiWindows {
        fn leggi_testo(&self, percorso: &str, nome: &str) -> Result<Option<String>, String> {
            Ok(leggi_grezzo(percorso, nome, RRF_RT_REG_SZ)?.map(|byte| da_utf16_con_zero(&byte)))
        }

        fn leggi_byte(&self, percorso: &str, nome: &str) -> Result<Option<Vec<u8>>, String> {
            leggi_grezzo(percorso, nome, RRF_RT_REG_BINARY)
        }

        #[allow(
            unsafe_code,
            reason = "RegSetKeyValueW non ha un'interfaccia sicura: il blocco dice \
                      perché è corretto"
        )]
        fn scrivi_testo(&self, percorso: &str, nome: &str, valore: &str) -> Result<(), String> {
            let dati = a_utf16_con_zero(valore);
            let Ok(misura) = u32::try_from(dati.len()) else {
                return Err("il valore è troppo lungo".to_owned());
            };
            let percorso = HSTRING::from(percorso);
            let nome = HSTRING::from(nome);
            // SAFETY: `dati` è un buffer nostro di `misura` byte, vivo per tutta
            // la chiamata, e `REG_SZ` vuole proprio UTF-16 col terminatore, come
            // lo costruisce `a_utf16_con_zero`; Windows lo copia e non lo
            // conserva. Le due `HSTRING` vivono per tutta la chiamata e sono
            // terminate da zero. La chiave, se manca, la crea lui.
            let esito = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    &percorso,
                    &nome,
                    REG_SZ.0,
                    Some(dati.as_ptr().cast()),
                    misura,
                )
            };
            if esito == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(errore("la scrittura", esito))
            }
        }

        #[allow(
            unsafe_code,
            reason = "RegDeleteKeyValueW non ha un'interfaccia sicura: il blocco dice \
                      perché è corretto"
        )]
        fn cancella(&self, percorso: &str, nome: &str) -> Result<(), String> {
            let percorso = HSTRING::from(percorso);
            let nome = HSTRING::from(nome);
            // SAFETY: due `HSTRING` nostre, vive per tutta la chiamata e
            // terminate da zero; nessun'altra memoria passa al sistema.
            let esito = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &percorso, &nome) };
            if esito == ERROR_SUCCESS || e_assente(esito) {
                Ok(())
            } else {
                Err(errore("la cancellazione", esito))
            }
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_testo_diventa_utf16_col_terminatore() {
        // «Ab»: due unità, più lo zero, a due byte l'una.
        assert_eq!(a_utf16_con_zero("Ab"), vec![0x41, 0, 0x62, 0, 0, 0]);
        // Una lettera fuori dal piano base occupa due unità.
        assert_eq!(a_utf16_con_zero("𝄞").len(), 6);
        assert_eq!(a_utf16_con_zero(""), vec![0, 0]);
    }

    #[test]
    fn il_testo_va_e_torna_anche_con_le_lettere_accentate() {
        for testo in [
            r#""C:\Users\Bjørk\AppData\Local\Rymembra\rymembra.exe" --avvio-automatico"#,
            "è così",
            "",
        ] {
            assert_eq!(da_utf16_con_zero(&a_utf16_con_zero(testo)), testo);
        }
    }

    #[test]
    fn la_lettura_si_ferma_al_primo_terminatore() {
        // Un `REG_SZ` può portarsi dietro byte dopo lo zero: non fanno parte
        // del testo.
        let mut byte = a_utf16_con_zero("ciao");
        byte.extend_from_slice(&[0x41, 0, 0x42, 0]);
        assert_eq!(da_utf16_con_zero(&byte), "ciao");
        // E un byte spaiato in fondo non fa un carattere.
        assert_eq!(da_utf16_con_zero(&[0x41, 0, 0x42]), "A");
    }
}
