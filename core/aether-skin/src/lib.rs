//! Il motore delle skin: da `skin.json` a CSS, e il pacchetto `.aeskin`.
//!
//! # La regola che spiega tutto il resto
//!
//! **Una skin è dati, non CSS.** Il compilatore è l'unico autore di CSS del
//! sistema, e nessun valore scritto da un autore viene mai copiato nell'output:
//! un colore entra come canali, una lunghezza come numero più unità, una curva
//! come quattro numeri.
//!
//! La conseguenza pratica: l'intera classe di attacchi via CSS — `url()` che
//! chiama a casa, `@import` che carica un foglio remoto, selettori che
//! esfiltrano il contenuto degli attributi — non si applica, perché non esiste
//! un canale in cui infilarli. Ma la garanzia vale **solo** se ogni campo è
//! tipizzato: basta uno che accetti una stringa e la copi, e salta tutta
//! insieme. Per questo la validazione rifiuta anche cose innocue come `calc()` e
//! i nomi di colore CSS.
//!
//! # Perché sta nel nucleo, in Rust
//!
//! Nel vecchio albero questo motore esisteva in TypeScript e girava nel
//! renderer. Portarlo qui non è una traduzione: è ciò che permette allo stesso
//! codice di validare un pacchetto sul desktop e dentro l'APK. Un pacchetto
//! ricevuto dal telefono passa dalle stesse guardie di uno aperto sul PC —
//! stessa lista di nomi ammessi, stesso tetto sul rapporto di compressione,
//! stesse firme dei tipi — invece che da due implementazioni che possono
//! divergere. È lo stesso motivo per cui esiste `aether-domain`.
//!
//! Il CSS che esce di qui lo adotta la finestra; su Android servirà la stessa
//! struttura tradotta in temi Compose, e la parte che decide — token, effetti,
//! costi, validazione — sarà già scritta una volta sola.
//!
//! # Come si legge questo crate
//!
//! - [`values`]: cosa può valere un campo. È la sicurezza del formato.
//! - [`tokens`]: il contratto fra le skin e i componenti.
//! - [`effects`]: gli undici effetti, e quanto costano.
//! - [`parts`]: le superfici ridisegnabili.
//! - [`document`]: la forma di `skin.json`, e la validazione.
//! - [`compile`]: l'unico autore di CSS.
//! - [`package`]: il formato `.aeskin`, e le difese contro un archivio ostile.
//!
//! # Come si aggiunge un token
//!
//! Si aggiunge la voce a [`tokens::TOKENS`] col suo nome CSS, il tipo, il gruppo
//! e una descrizione utile a chi la leggerà nell'editor. Non serve altro:
//! validazione e compilatore si derivano dal registro. Se il token è
//! obbligatorio, la skin di riferimento deve dichiararlo — è quello che il test
//! di fedeltà verifica.
//!
//! # Come si aggiunge un effetto
//!
//! Si aggiunge la variante a [`effects::Effect`]. Il costo, la proprietà di
//! destinazione, il nome e la compilazione sono quattro `match` esaustivi: il
//! compilatore chiede tutti e quattro, e un effetto a metà non si compila.

pub mod compile;
pub mod document;
pub mod effects;
pub mod package;
pub mod parts;
pub mod tokens;
pub mod values;
mod vicini;

pub use compile::{CompiledSkin, compile_skin};
pub use document::{
    SKIN_FORMAT_VERSION, SkinDocument, SkinIssue, SkinWarning, check_skin, parse_skin,
    parse_skin_json,
};
pub use effects::{Effect, SURFACE_COST_BUDGET, exceeds_budget, stack_cost};
pub use package::{SkinPackage, read_skin_package, write_skin_package};
pub use tokens::{TOKENS, TokenDef};

/// La skin di riferimento, come è scritta.
///
/// Non è una skin nuova: è il blocco `:root` di `global.css` riga per riga.
/// Serve da collaudo — un formato provato solo su casi costruiti per riuscire è
/// un formato che non si sa se regge, e convertire `plain` ha fatto emergere la
/// lunghezza adattiva, `color-scheme` e i riferimenti fra token.
pub const PLAIN_SOURCE: &str = include_str!("../skins/plain.json");

/// La skin di riferimento, validata.
///
/// # Errori
///
/// Solo se `skins/plain.json` non è valido, il che è un guasto nostro e lo dice
/// il test di fedeltà prima di chiunque altro.
pub fn plain() -> Result<SkinDocument, aether_domain::AppError> {
    parse_skin_json(PLAIN_SOURCE)
}
