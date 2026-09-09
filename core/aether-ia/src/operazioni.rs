//! Il protocollo delle modifiche: un blocco delimitato dentro la risposta.
//!
//! # Perché non il tool-calling
//!
//! Perché non funziona dove serve. Il tool-calling è la strada giusta con i
//! modelli grandi dei fornitori grandi; con un modello da otto miliardi di
//! parametri servito da Ollama è un campo che qualche volta arriva, qualche
//! volta arriva come stringa invece che come oggetto, e qualche volta arriva
//! scritto dentro il testo perché il modello ha imitato il formato senza usare
//! il canale. Un blocco di codice delimitato, invece, lo sanno produrre tutti:
//! è la cosa che hanno visto più spesso durante l'addestramento.
//!
//! Il blocco ha questa forma, e la sua descrizione sta nelle istruzioni che
//! [`crate::Cliente`] riceve già scritte:
//!
//! ````text
//! ```aether-patch
//! [{"op": "scrivi", "percorso": ["tokens", "color.surface.0"], "valore": "#050508"},
//!  {"op": "scrivi", "percorso": ["parts", "section-card", "background", "0", "stops", "1", "color"], "valore": "#1a1512"},
//!  {"op": "togli",  "percorso": ["parts", "section-card", "radius"]}]
//! ```
//! ````
//!
//! # Perché l'estrazione sta in Rust
//!
//! Per due ragioni, e la seconda è quella che decide. La prima è che qui si
//! può provare: `npm run verify` non ha prove TypeScript, e questa è la
//! funzione che riceve testo scritto da un modello — cioè l'ingresso meno
//! prevedibile di tutta l'applicazione. La seconda è che la finestra non deve
//! mai essere il posto in cui si decide **se** una modifica è ben formata: lo
//! decide il nucleo, e la finestra disegna quel che il nucleo ha già accettato.
//!
//! # Cosa questo modulo continua a non sapere
//!
//! Cos'è una skin. `percorso` è una sequenza di passi dentro un documento JSON
//! qualunque, e `tokens` o `parts` sono stringhe come le altre. Il giorno in cui
//! qui dentro comparisse la parola «token» con un significato, questo crate
//! avrebbe smesso di essere un client di modelli.

use serde::Deserialize;
use serde_json::Value;

/// Il delimitatore che apre il blocco.
///
/// Il linguaggio si chiama `aether-patch` e non `json` di proposito: un modello
/// che spiega quel che sta per fare scrive volentieri anche un blocco `json`
/// d'esempio, e due blocchi indistinguibili vorrebbero dire applicare
/// l'esempio.
const LINGUAGGIO: &str = "aether-patch";

/// Quanti passi può avere un percorso.
///
/// Dodici. Otto era il conto di un documento fatto di soli oggetti, e sarebbe
/// ancora giusto se un manifest lo fosse: un aspetto di uno stato di una parte
/// sta in quattro passi. Ma le pile costano due passi per livello, e
/// `parts.x.states.hover.background.0.stops.1.color` è già otto esatti — cioè
/// il tetto che scatta sulla modifica più normale che si possa chiedere a un
/// modello, invece che sull'errore che deve fermare. Un nodo di scafale
/// annidato tre volte è nove.
///
/// Dodici tiene quei casi e ferma ancora quel che il numero esiste per fermare:
/// un percorso da cinquanta passi non è una modifica, è un modello che si è
/// impuntato.
const PASSI_MASSIMI: usize = 12;

/// Quanto di un valore storto entra in una ragione. Vedi [`corto`].
const RAGIONE_MASSIMA: usize = 120;

/// Cosa fare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Mettere un valore lì, creando quel che manca.
    Scrivi,
    /// Togliere quel che c'è lì.
    Togli,
}

/// Una modifica sola.
#[derive(Debug, Clone, PartialEq)]
pub struct Operazione {
    /// Cosa fare.
    pub op: Op,
    /// Dove, un passo per livello.
    pub percorso: Vec<String>,
    /// Il valore, per [`Op::Scrivi`] e solo per quella.
    pub valore: Option<Value>,
}

/// La forma grezza, prima dei controlli.
///
/// # Perché i nomi inglesi si accettano
///
/// Perché un modello che ha letto diecimila JSON Patch scrive JSON Patch, e
/// l'ha fatto davvero alla seconda risposta di un modello locale: `path` invece
/// di `percorso`, `value` invece di `valore`, `replace` invece di `scrivi`. In
/// un elenco di operazioni quei tre nomi non sono ambigui, e rifiutarli
/// costerebbe un giro di correzione per una parola. Il protocollo resta quello
/// italiano — è quel che le istruzioni chiedono e quel che le prove fissano;
/// questi sono ripieghi che si leggono, non forme che si insegnano.
///
/// Separata da [`Operazione`] perché serve a rappresentare anche quel che è
/// **sbagliato**: un `Deserialize` su `Operazione` con `percorso: Vec<String>`
/// farebbe fallire l'intero elenco per una voce storta, e una risposta a metà
/// non deve valere zero.
#[derive(Debug, Deserialize)]
struct Grezza {
    op: Option<String>,
    /// Un `Value` e non un `Vec<Value>`: un percorso scritto come stringa è lo
    /// sbaglio più comune di un modello piccolo, e con il tipo stretto farebbe
    /// fallire la **deserializzazione dell'intero blocco** — nove operazioni
    /// buone perse per una, e un messaggio che parla del blocco invece che del
    /// campo.
    #[serde(alias = "path")]
    percorso: Option<Value>,
    #[serde(alias = "value")]
    valore: Option<Value>,
}

/// Quanto di un valore storto si ripete dentro una ragione.
///
/// Le ragioni si mostrano a chi guarda e tornano al modello: un percorso
/// sbagliato lungo diecimila caratteri riempirebbe tutti e due senza aggiungere
/// niente a quel che la prima riga dice già.
fn corto(cosa: &str) -> String {
    match cosa.char_indices().nth(RAGIONE_MASSIMA) {
        None => cosa.to_owned(),
        Some((fine, _)) => format!("{}…", cosa.get(..fine).unwrap_or_default()),
    }
}

/// Le operazioni ben formate dentro un testo, e le ragioni di quel che si è
/// scartato.
///
/// # Perché quel che è storto si nomina invece di far fallire tutto
///
/// Perché una risposta con nove modifiche buone e una storta è una risposta
/// utile, e buttarla via costringerebbe a rifare la stessa domanda sperando in
/// un'altra fortuna. Le ragioni tornano indietro per due usi: mostrarle a chi
/// guarda, e rimandarle al modello in modalità agent, dove sono esattamente
/// l'informazione che gli serve per correggersi.
///
/// Se non c'è nessun blocco l'elenco è vuoto e le ragioni pure: una risposta
/// che spiega qualcosa senza proporre modifiche non è un errore.
#[must_use]
pub fn estrai(testo: &str) -> (Vec<Operazione>, Vec<String>) {
    let mut operazioni = Vec::new();
    let mut ragioni = Vec::new();

    for (indice, blocco) in blocchi(testo).into_iter().enumerate() {
        let grezze: Vec<Grezza> = match serde_json::from_str(blocco.trim()) {
            Ok(elenco) => elenco,
            Err(err) => {
                ragioni.push(format!(
                    "il blocco {} non è un elenco JSON valido: {err}",
                    indice.saturating_add(1)
                ));
                continue;
            }
        };
        for (n, grezza) in grezze.into_iter().enumerate() {
            // Il percorso si legge **prima** di consumare la voce, e finisce
            // nella ragione accanto al numero. Il numero da solo dice al modello
            // quale contare, non quale riscrivere: visto davvero, un modello
            // locale che, ricevuto un rifiuto senza percorso, ha risposto di non
            // sapere più quale fosse l'operazione da rifare e ha chiesto di
            // ripetergliela — cioè un giro di correzione speso a domandare.
            let dove = grezza.percorso.as_ref().map(ToString::to_string);
            match controlla(grezza) {
                Ok(operazione) => operazioni.push(operazione),
                Err(perche) => {
                    let dove = dove.map_or_else(String::new, |p| format!(" su {}", corto(&p)));
                    ragioni.push(format!(
                        "l'operazione {}{dove} del blocco {}: {perche}",
                        n.saturating_add(1),
                        indice.saturating_add(1)
                    ));
                }
            }
        }
    }

    (operazioni, ragioni)
}

/// I corpi dei blocchi `aether-patch` presenti nel testo.
///
/// Scritto a mano e non con un'espressione regolare perché il workspace non ha
/// un motore di espressioni regolari e non vale portarne uno per sei righe —
/// ma soprattutto perché la regola vera non è «tre apici»: è «tre apici
/// **all'inizio di una riga**», e un blocco delimitato dentro il quale il
/// modello ha scritto tre apici in mezzo a una riga non deve chiudersi lì.
fn blocchi(testo: &str) -> Vec<&str> {
    let mut fuori = Vec::new();
    let mut dentro: Option<usize> = None;
    let mut posizione = 0_usize;

    for riga in testo.split_inclusive('\n') {
        let inizio = posizione;
        posizione = posizione.saturating_add(riga.len());
        let potata = riga.trim();
        match dentro {
            None => {
                // `strip_prefix` e non `contains`: «vedi il blocco ```aether-patch»
                // dentro una frase è una frase, non un blocco che si apre.
                if let Some(coda) = potata.strip_prefix("```")
                    && coda.trim() == LINGUAGGIO
                {
                    dentro = Some(posizione);
                }
            }
            Some(da) => {
                if potata.starts_with("```") {
                    if let Some(corpo) = testo.get(da..inizio) {
                        fuori.push(corpo);
                    }
                    dentro = None;
                }
            }
        }
    }
    // Un blocco aperto e mai chiuso è quel che si ottiene quando il modello
    // finisce i gettoni a metà. Si prende com'è: il JSON dentro sarà troncato e
    // lo dirà il parser, con un messaggio più utile di «manca la chiusura».
    if let Some(da) = dentro
        && let Some(corpo) = testo.get(da..)
    {
        fuori.push(corpo);
    }
    fuori
}

/// La forma grezza diventa un'operazione, o dice perché no.
fn controlla(grezza: Grezza) -> Result<Operazione, String> {
    // I nomi italiani sono il protocollo, e restano quelli. Quelli inglesi si
    // accettano perché le istruzioni che il modello legge sono **in inglese** —
    // vedi il preambolo di `prompt.ts`, dove sta scritto perché — e un modello
    // che ha letto un paragrafo inglese scrive «set». Rifiutarlo costerebbe un
    // giro di correzione per una parola che non è ambigua: è la stessa scelta
    // dei numeri nel percorso, poco più in basso.
    let op = match grezza.op.as_deref() {
        Some("scrivi" | "set" | "replace") => Op::Scrivi,
        // `remove` e `delete` fanno quel che fa «togli» anche in JSON Patch: su
        // una posizione tolgono, e il resto scala.
        Some("togli" | "remove" | "delete") => Op::Togli,
        // I nomi con cui si aggiunge a un elenco, e sono l'unica famiglia che va
        // **rifiutata invece che tradotta**. `add` in JSON Patch, su una
        // posizione, vuol dire «inserisci prima»; `push` e `append` vogliono dire
        // «in fondo» e non portano nemmeno una posizione. Mapparli su «scrivi»
        // sostituirebbe in silenzio il livello che si voleva spingere in giù,
        // oppure scriverebbe sopra l'elenco intero.
        //
        // Rifiutarli e basta non basterebbe: sono i nomi che un modello inventa
        // proprio quando l'operazione che gli serve **c'è** e si chiama
        // altrimenti — `push`, visto davvero, su un modello locale a cui era
        // stato chiesto di aggiungere un livello. Quindi la ragione non dice solo
        // di no: dice come si fa, ed è quel che in modalità agent basta al giro
        // dopo.
        Some(nome @ ("add" | "push" | "append" | "insert" | "prepend")) => {
            return Err(format!(
                "«{nome}» non esiste: per aggiungere in fondo a un elenco usa «scrivi» con, come ultimo passo del percorso, la posizione pari alla lunghezza attuale dell'elenco; per inserire in mezzo o riordinare, riscrivi l'elenco intero con una «scrivi» sola sull'elenco"
            ));
        }
        Some(altro) => {
            return Err(format!("«{}» non è né «scrivi» né «togli»", corto(altro)));
        }
        None => return Err("manca il campo «op»".to_owned()),
    };

    let passi = match grezza.percorso {
        Some(Value::Array(passi)) => passi,
        // Un percorso scritto come stringa — `parts.section-card.background[0]`
        // — qui non si può sciogliere, e non è pigrizia: i nomi dei token
        // contengono dei punti, quindi `tokens.color.surface.0` è **due** passi
        // e non quattro, e per saperlo bisogna guardare il documento. Questo
        // modulo il documento non ce l'ha e non deve averlo. La ragione porta
        // allora la forma giusta, che è quel che serve al giro dopo.
        Some(Value::String(scritto)) => {
            return Err(format!(
                "«percorso» è la stringa «{}»: dev'essere un elenco di passi, uno per livello, per esempio [\"parts\", \"section-card\", \"background\", \"0\", \"stops\", \"0\", \"color\"]",
                corto(&scritto)
            ));
        }
        Some(altro) => {
            return Err(format!(
                "«percorso» non è un elenco di passi: {}",
                corto(&altro.to_string())
            ));
        }
        None => return Err("manca il campo «percorso»".to_owned()),
    };
    if passi.is_empty() {
        return Err("il percorso è vuoto: non dice dove".to_owned());
    }
    if passi.len() > PASSI_MASSIMI {
        return Err(format!(
            "il percorso ha {} passi, il massimo è {PASSI_MASSIMI}",
            passi.len()
        ));
    }
    let mut percorso = Vec::with_capacity(passi.len());
    for passo in passi {
        // I numeri si accettano e diventano testo: un modello che scrive
        // `["background", 0]` intende il passo «0», e rifiutarlo per il tipo
        // sarebbe pignoleria su una cosa che non è ambigua. Cosa poi voglia
        // dire quel passo lo decide chi applica: dentro un elenco è una
        // posizione, dentro un oggetto è il nome di una chiave. Qui non c'è
        // nessun documento da guardare, e non c'è niente da decidere.
        let passo = match passo {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            altro => {
                return Err(format!(
                    "un passo del percorso non è testo: {}",
                    corto(&altro.to_string())
                ));
            }
        };
        if passo.is_empty() {
            return Err("un passo del percorso è vuoto".to_owned());
        }
        percorso.push(passo);
    }

    // Il valore è obbligatorio per «scrivi» e vietato per «togli», e la seconda
    // metà conta quanto la prima: un «togli» con dentro un valore è un modello
    // che intendeva «scrivi», e applicarlo cancellerebbe quel che voleva
    // mettere.
    match (op, grezza.valore) {
        (Op::Scrivi, Some(valore)) => Ok(Operazione {
            op,
            percorso,
            valore: Some(valore),
        }),
        (Op::Scrivi, None) => Err("«scrivi» senza il campo «valore»".to_owned()),
        (Op::Togli, None) => Ok(Operazione {
            op,
            percorso,
            valore: None,
        }),
        (Op::Togli, Some(_)) => Err("«togli» con un «valore»: forse era «scrivi»".to_owned()),
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn blocco(dentro: &str) -> String {
        format!("Ecco:\n\n```{LINGUAGGIO}\n{dentro}\n```\n\nFatto.")
    }

    #[test]
    fn un_blocco_normale_esce_intero() {
        // Due cancelletti: il valore di prova è un colore, e `"#` chiuderebbe
        // un raw string a un cancelletto solo.
        let testo = blocco(
            r##"[{"op": "scrivi", "percorso": ["tokens", "color.surface.0"], "valore": "#050508"},
                {"op": "togli", "percorso": ["parts", "section-card", "radius"]}]"##,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(operazioni.len(), 2);
        assert_eq!(operazioni[0].op, Op::Scrivi);
        assert_eq!(operazioni[0].percorso, ["tokens", "color.surface.0"]);
        assert_eq!(operazioni[0].valore, Some(Value::from("#050508")));
        assert_eq!(operazioni[1].op, Op::Togli);
        assert_eq!(operazioni[1].valore, None);
    }

    #[test]
    fn senza_blocco_non_succede_niente() {
        let (operazioni, ragioni) = estrai("Non ho capito la richiesta, puoi ripetere?");
        assert!(operazioni.is_empty());
        assert!(ragioni.is_empty());
    }

    /// Il motivo per cui il linguaggio non si chiama `json`: un modello che
    /// spiega quel che fa scrive volentieri anche un esempio.
    #[test]
    fn un_blocco_json_di_esempio_non_si_applica() {
        let testo = format!(
            "Adesso i token stanno così:\n\n```json\n{}\n```\n\ne li cambio:\n\n```{LINGUAGGIO}\n{}\n```\n",
            r#"[{"op": "scrivi", "percorso": ["tokens", "sbagliato"], "valore": 1}]"#,
            r#"[{"op": "scrivi", "percorso": ["tokens", "giusto"], "valore": 2}]"#,
        );
        let (operazioni, _) = estrai(&testo);
        assert_eq!(operazioni.len(), 1);
        assert_eq!(operazioni[0].percorso, ["tokens", "giusto"]);
    }

    /// Nove buone e una storta valgono nove, non zero.
    #[test]
    fn quel_che_e_storto_si_nomina_e_il_resto_passa() {
        let testo = blocco(
            r#"[{"op": "scrivi", "percorso": ["a"], "valore": 1},
                {"op": "cancella", "percorso": ["b"]},
                {"op": "scrivi", "percorso": []},
                {"op": "togli", "percorso": ["c"], "valore": 3},
                {"op": "togli", "percorso": ["d"]}]"#,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert_eq!(operazioni.len(), 2, "{operazioni:?}");
        assert_eq!(ragioni.len(), 3, "{ragioni:?}");
        assert!(ragioni[0].contains("cancella"), "{ragioni:?}");
        // Il percorso accanto al numero: dice **quale** operazione riscrivere.
        assert!(ragioni[0].contains("[\"b\"]"), "{ragioni:?}");
        assert!(ragioni[1].contains("percorso"), "{ragioni:?}");
        assert!(ragioni[2].contains("scrivi"), "{ragioni:?}");
    }

    #[test]
    fn un_json_rotto_dice_dove() {
        let (operazioni, ragioni) = estrai(&blocco("[{\"op\": \"scrivi\","));
        assert!(operazioni.is_empty());
        assert_eq!(ragioni.len(), 1);
        assert!(ragioni[0].contains("blocco 1"), "{ragioni:?}");
    }

    /// Quel che si ottiene quando il modello finisce i gettoni a metà blocco.
    #[test]
    fn un_blocco_mai_chiuso_si_prende_lo_stesso() {
        let testo = format!("```{LINGUAGGIO}\n[{{\"op\": \"scrivi\", \"percorso\": [\"a\"]");
        let (operazioni, ragioni) = estrai(&testo);
        assert!(operazioni.is_empty());
        assert_eq!(ragioni.len(), 1, "{ragioni:?}");
    }

    /// I nomi inglesi: le istruzioni che il modello legge sono in inglese, e un
    /// modello che ha letto un paragrafo inglese scrive «set». Visto davvero,
    /// alla prima risposta di un modello locale.
    #[test]
    fn i_nomi_inglesi_delle_operazioni_valgono_quelli_italiani() {
        let testo = blocco(
            r#"[{"op": "set", "percorso": ["a"], "valore": 1},
                {"op": "remove", "percorso": ["b"]},
                {"op": "delete", "percorso": ["c"]}]"#,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(operazioni.len(), 3);
        assert_eq!(operazioni[0].op, Op::Scrivi);
        assert_eq!(operazioni[1].op, Op::Togli);
        assert_eq!(operazioni[2].op, Op::Togli);
    }

    /// JSON Patch per intero, che è quel che un modello scrive quando scivola
    /// sulla forma che ha visto diecimila volte. Visto davvero, alla seconda
    /// risposta di un modello locale.
    #[test]
    fn la_forma_di_json_patch_si_legge_lo_stesso() {
        let testo = blocco(
            r##"[{"op": "replace", "path": ["parts", "section-card", "background", "0", "stops", "0", "color"], "value": "#e3a44f"}]"##,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(operazioni.len(), 1);
        assert_eq!(operazioni[0].op, Op::Scrivi);
        assert_eq!(
            operazioni[0].percorso,
            [
                "parts",
                "section-card",
                "background",
                "0",
                "stops",
                "0",
                "color"
            ]
        );
        assert_eq!(operazioni[0].valore, Some(Value::from("#e3a44f")));
    }

    /// I nomi con cui si aggiunge a un elenco: gli unici che vanno rifiutati
    /// invece che tradotti, perché mapparli su «scrivi» sostituirebbe in
    /// silenzio quel che si voleva spostare. `push` l'ha scritto davvero un
    /// modello locale a cui era stato chiesto di aggiungere un livello.
    #[test]
    fn i_nomi_per_aggiungere_non_si_accettano_e_dicono_cosa_fare() {
        for nome in ["add", "push", "append", "insert", "prepend"] {
            let testo = blocco(&format!(
                r#"[{{"op": "{nome}", "path": ["parts", "x", "background"], "value": {{}}}}]"#
            ));
            let (operazioni, ragioni) = estrai(&testo);
            assert!(operazioni.is_empty(), "{nome}: {operazioni:?}");
            assert_eq!(ragioni.len(), 1, "{nome}: {ragioni:?}");
            assert!(ragioni[0].contains(nome), "{nome}: {ragioni:?}");
            assert!(ragioni[0].contains("lunghezza"), "{nome}: {ragioni:?}");
            assert!(ragioni[0].contains("riscrivi"), "{nome}: {ragioni:?}");
        }
    }

    /// Il percorso scritto come stringa: lo sbaglio più comune di un modello
    /// piccolo, e quello che prima portava via tutto il blocco. Qui vale una
    /// voce sola, e la ragione porta la forma giusta — che in modalità agent
    /// è esattamente quel che serve al giro dopo.
    #[test]
    fn un_percorso_scritto_come_stringa_si_nomina_e_non_porta_via_il_blocco() {
        // Due cancelletti, come sopra: il valore di prova è un colore, e `"#`
        // chiuderebbe un raw string a un cancelletto solo.
        let testo = blocco(
            r##"[{"op": "scrivi", "percorso": "parts.section-card.background[0].stops[0].color", "valore": "#fff"},
                 {"op": "togli", "percorso": ["parts", "np-screen", "radius"]}]"##,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert_eq!(operazioni.len(), 1, "la seconda passa: {operazioni:?}");
        assert_eq!(operazioni[0].op, Op::Togli);
        assert_eq!(ragioni.len(), 1, "{ragioni:?}");
        assert!(ragioni[0].contains("elenco di passi"), "{ragioni:?}");
        assert!(ragioni[0].contains("section-card"), "{ragioni:?}");
    }

    /// Un percorso che non è Nè un elenco Nè una stringa non fa saltare
    /// il blocco: si nomina come tutto il resto.
    #[test]
    fn un_percorso_di_un_altro_tipo_si_nomina() {
        let testo = blocco(r#"[{"op": "togli", "percorso": 7}]"#);
        let (operazioni, ragioni) = estrai(&testo);
        assert!(operazioni.is_empty());
        assert_eq!(ragioni.len(), 1, "{ragioni:?}");
        assert!(ragioni[0].contains("percorso"), "{ragioni:?}");
    }

    /// Le ragioni si mostrano e tornano al modello: un valore storto lungo
    /// diecimila caratteri non deve riempire Nè l'uno Nè l'altro.
    #[test]
    fn una_ragione_non_ripete_diecimila_caratteri() {
        let lunga = "x".repeat(10_000);
        let testo = blocco(&format!(r#"[{{"op": "{lunga}", "percorso": ["a"]}}]"#));
        let (_, ragioni) = estrai(&testo);
        assert_eq!(ragioni.len(), 1, "{ragioni:?}");
        assert!(ragioni[0].chars().count() < 200, "{}", ragioni[0].len());
        // I puntini stanno dove finisce il valore, non in fondo alla frase: la
        // ragione continua a dire cosa non andava.
        assert!(ragioni[0].contains('…'), "{ragioni:?}");
    }

    #[test]
    fn i_numeri_nel_percorso_diventano_passi() {
        let testo = blocco(r#"[{"op": "togli", "percorso": ["parts", 0]}]"#);
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(operazioni[0].percorso, ["parts", "0"]);
    }

    /// Il percorso più lungo che una modifica vera possa avere: un aspetto di
    /// uno stato di una parte, dentro una pila, dentro un gradiente. Se il tetto
    /// scattasse qui, scatterebbe sulla richiesta più normale invece che
    /// sull'errore.
    #[test]
    fn un_percorso_con_due_posizioni_passa() {
        let testo = blocco(
            r##"[{"op": "scrivi", "percorso": ["parts", "section-card", "states", "hover", "background", 0, "stops", 1, "color"], "valore": "#1a1512"}]"##,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(operazioni.len(), 1);
        assert_eq!(
            operazioni[0].percorso,
            [
                "parts",
                "section-card",
                "states",
                "hover",
                "background",
                "0",
                "stops",
                "1",
                "color"
            ]
        );
    }

    #[test]
    fn un_percorso_troppo_profondo_si_ferma() {
        let passi: Vec<String> = (0..=PASSI_MASSIMI).map(|n| format!("\"p{n}\"")).collect();
        let testo = blocco(&format!(
            "[{{\"op\": \"togli\", \"percorso\": [{}]}}]",
            passi.join(",")
        ));
        let (operazioni, ragioni) = estrai(&testo);
        assert!(operazioni.is_empty());
        assert_eq!(ragioni.len(), 1, "{ragioni:?}");
    }

    /// Tre apici in mezzo a una riga di JSON non chiudono niente.
    #[test]
    fn il_delimitatore_e_solo_a_inizio_riga() {
        let testo = blocco(r#"[{"op": "scrivi", "percorso": ["a"], "valore": "x ``` y"}]"#);
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(operazioni[0].valore, Some(Value::from("x ``` y")));
    }

    /// Il valore può essere un oggetto: `{"$token": …}` è un valore legale del
    /// documento di una skin, e questo modulo non deve saperlo per lasciarlo
    /// passare.
    #[test]
    fn un_valore_puo_essere_qualunque_json() {
        let testo = blocco(
            r#"[{"op": "scrivi", "percorso": ["parts", "x", "bg"], "valore": {"$token": "color.surface.1"}}]"#,
        );
        let (operazioni, ragioni) = estrai(&testo);
        assert!(ragioni.is_empty(), "{ragioni:?}");
        assert_eq!(
            operazioni[0].valore,
            Some(serde_json::json!({"$token": "color.surface.1"}))
        );
    }
}
