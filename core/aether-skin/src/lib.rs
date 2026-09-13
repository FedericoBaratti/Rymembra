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
//! - [`preset`]: blocchi di valori pronti, per chi non vuole muovere quindici
//!   cursori.
//! - [`effects`]: gli undici effetti, e quanto costano.
//! - [`movimento`]: le animazioni nominate, e quanto costano.
//! - [`parts`]: le superfici ridisegnabili.
//! - [`layout`]: dove stanno le cose — il vocabolario dello scafale.
//! - [`document`]: la forma di `skin.json`, e la validazione.
//! - [`posizioni`]: dal percorso di un problema alla riga in cui è scritto.
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
//! # Come si aggiunge un preset
//!
//! Si aggiunge la voce a [`preset::PRESETS`] con il suo gruppo e l'elenco dei
//! token che scrive, e ogni valore si scrive **com'è nel documento**: un
//! frammento JSON dentro una stringa. Non serve altro nemmeno qui — la striscia
//! nello Studio si costruisce dal gruppo del token selezionato, e non nomina
//! nessun preset.
//!
//! La regola che vale la pena ricordare è la seconda: un preset scrive soltanto
//! quel che deve **cambiare**. Ripetere un valore che è già quello di serie non
//! rende il preset più esplicito, lo rende una seconda copia di un numero che
//! vive nel registro — cioè la prima cosa che divergerà il giorno che quel
//! numero cambia. Sono due prove: una fonde ogni preset in [`PLAIN_SOURCE`] e
//! pretende una skin valida e senza avvisi, l'altra che ogni token nominato
//! esista davvero e appartenga al gruppo dichiarato.
//!
//! # Come si aggiunge un effetto
//!
//! Si aggiunge la variante a [`effects::Effect`]. Il costo, la proprietà di
//! destinazione, il nome e la compilazione sono quattro `match` esaustivi: il
//! compilatore chiede tutti e quattro, e un effetto a metà non si compila.
//!
//! # Come si anima una parte
//!
//! L'animazione si dichiara **una volta** in `motion.animations`, con un nome,
//! e la parte la richiama in `parts.<parte>.animations.<trigger>`. È la stessa
//! forma dei motivi e delle curve, e per la stessa ragione: un valore scritto
//! dove si usa è un valore che si ripete. Le due regole che
//! [`movimento`] non lascia aggirare — ogni durata dentro
//! `calc(… * var(--motion-scale, 1))`, e nessuna ripetizione infinita — non
//! sono di stile: sono ciò che rende l'animazione di una skin fermabile da chi
//! ha chiesto meno movimento.

pub mod compile;
pub mod dinamico;
pub mod document;
pub mod effects;
pub mod layout;
pub mod movimento;
pub mod package;
pub mod parts;
pub mod posizioni;
pub mod preset;
pub mod tokens;
pub mod values;
mod vicini;

pub use compile::{CompiledSkin, compile_skin};
pub use dinamico::{Oklch, accento_sicuro};
pub use document::{
    CONTRASTO_MINIMO, ContrastPair, SKIN_FORMAT_VERSION, SkinDocument, SkinIssue, SkinWarning,
    WarningKind, check_skin, contrast_pairs, leggi_skin, palette_usage, parse_skin,
    parse_skin_json,
};
pub use effects::{Effect, SURFACE_COST_BUDGET, exceeds_budget, stack_cost};
pub use layout::{
    LayoutNode, LayoutZone, SHELL_COST_BUDGET, WIDGETS, WidgetDef, WidgetInstance, default_shell,
};
pub use movimento::{
    AnimDirection, AnimFrame, AnimTrigger, Animation, AnimationRef, MOTION_COST_BUDGET,
    PartAnimations, animation_cost, exceeds_motion_budget, motion_cost,
};
pub use package::{SkinPackage, read_skin_package, write_skin_package};
pub use posizioni::{Posizioni, Punto, posizioni};
pub use preset::{PRESETS, PresetDef};
pub use tokens::{TOKENS, TokenDef};

/// La skin di riferimento, come è scritta.
///
/// Non è una skin nuova: è il blocco `:root` di `global.css` riga per riga.
/// Serve da collaudo — un formato provato solo su casi costruiti per riuscire è
/// un formato che non si sa se regge, e convertire `plain` ha fatto emergere la
/// lunghezza adattiva, `color-scheme` e i riferimenti fra token.
///
/// Due voci non vengono da `global.css` e sono del guscio nuovo: `font.mono`,
/// che serve a durate e indici, e il ciano della tavolozza, che disegna lo
/// spinner e la barra di avanzamento. Il ciano sta nella **tavolozza** e non fra
/// i token perché nessun componente lo legge per nome: è un colore che la skin
/// usa per sé, ed è esattamente il caso per cui la tavolozza locale esiste.
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
