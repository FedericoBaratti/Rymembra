//! La sincronia fra dispositivi: un documento per dispositivo, e la fusione che
//! li fa convergere.
//!
//! # Il problema, detto per bene
//!
//! Due computer e un telefono, nessun server, e nessuna garanzia su chi parla
//! con chi né in che ordine. Ognuno ascolta, vota, mette cuoricini e riordina
//! playlist per conto suo, magari mentre è scollegato. Quando si rivedono, deve
//! venirne fuori una sola verità — e deve venirne fuori **la stessa** su tutti e
//! tre, qualunque sia l'ordine in cui si sono parlati.
//!
//! # Perché uno stato per dispositivo e non un giornale di operazioni
//!
//! Un giornale append-only è la risposta che viene per prima, ed è quella che
//! cresce per sempre: la sua dimensione è quella della *storia*, non della
//! libreria, e prima o poi bisogna compattarlo — cioè bisogna decidere quando
//! due dispositivi hanno visto abbastanza da poter dimenticare, che è
//! esattamente la domanda a cui senza un server non si sa rispondere.
//!
//! Uno stato per dispositivo costa quanto la libreria e basta. Non si compatta
//! perché non cresce, un dispositivo sparito per un anno non obbliga nessuno a
//! conservare niente, e la fusione è una piega su una funzione che in questo
//! albero è già scritta e già provata: [`aether_domain::merge::merge_stats`].
//!
//! # Perché nessuno scrive il file di un altro
//!
//! ```text
//! <radice>/
//!   dispositivi/aether-<id>.v1.json.gz    ← solo <id> lo scrive; tutti lo leggono
//! ```
//!
//! Un Drive o un Dropbox produce un «conflicted copy» quando due processi
//! toccano lo stesso file. Con un file per dispositivo quel caso non esiste — non
//! è gestito meglio, proprio non si presenta — e la stessa disposizione funziona
//! identica su una cartella condivisa, su Drive e su S3, che è il motivo per cui
//! [`Magazzino`] ha cinque operazioni e non venti.
//!
//! # Le quattro forme di fusione, e perché sono quattro
//!
//! - **Conteggio** ([`contatore`]): ogni dispositivo fa salire soltanto il
//!   proprio numero, e il totale è la somma. Un massimo — che è quel che fa
//!   `merge_stats`, per una ragione sua e giusta — direbbe che cinque ascolti sul
//!   portatile e tre sul telefono fanno cinque.
//! - **Registro** ([`registro`]): voto, preferito, ultimo ascolto, punto di
//!   riascolto. Vince il più recente, con le stesse regole già scritte in
//!   `merge_stats`: uno zero non è un voto ma la sua assenza, e `liked_at` data
//!   la *decisione* e non il gradimento.
//! - **Sequenza** ([`sequenza`]): l'ordine di una playlist, che è l'unica cosa
//!   qui dentro per cui non basta scegliere un vincitore.
//! - **Lapidi**: una cancellazione è un fatto che deve viaggiare, o la cosa
//!   cancellata torna indietro dall'altro dispositivo alla prima passata.
//!
//! # Cosa questo crate non sa, e non è una dimenticanza
//!
//! Non conosce `rusqlite` e non conosce la rete. Riceve un [`documento`] già
//! costruito e ne restituisce uno fuso; chi ha il database lo riempie e lo
//! svuota, chi ha la rete gli passa un [`Magazzino`]. È la stessa regola per cui
//! `aether-cloud` non vede mai una `Connection` — «nessun lucchetto della
//! libreria resta preso durante una richiesta» è una proprietà che il
//! compilatore verifica, non un commento — e qui vale doppio, perché una
//! fusione va potuta provare senza né disco né rete, e infatti si prova così.

pub mod contatore;
pub mod documento;
pub mod fusione;
pub mod magazzino;
pub mod motore;
pub mod registro;
pub mod sequenza;

pub use contatore::Contatore;
pub use documento::{Contenuto, Documento, Lapidi, PlaylistSincronizzata, VERSIONE, impronta};
pub use fusione::{Fuso, fondi};
pub use magazzino::{Cartella, FileRemoto, Magazzino};
pub use motore::{Avanzamento, Esito, Guasto, Memoria, Motore, Passata};
pub use registro::{Interruttore, Momento, Registro, Scelta, Voto};
pub use sequenza::{Elemento, Identita, Sequenza};
