//! Quel che si legge dentro un indirizzo, senza andarlo a prendere.
//!
//! Un URL è una stringa, e alcune delle cose che se ne vogliono sapere si
//! decidono guardandola e basta: nessuna richiesta, nessuna rete, nessun
//! risultato che cambi fra una chiamata e l'altra. Sono decisioni, quindi
//! stanno qui — e chi la rete la apre davvero (`aether-net`, `aether-catalogo`)
//! la usa senza aprirla.
//!
//! Non è il posto in cui si dice *cosa* c'è a quell'indirizzo: quello lo
//! raccontano i tipi di [`crate::esterno`], che qualcuno ha dovuto riempire
//! parlando col mondo.

/// L'estensione suggerita da un indirizzo, in minuscolo e senza il punto.
///
/// Si guarda **solo** dentro il percorso, mai nell'host: `archive.org` finisce
/// con un punto e tre lettere esattamente come `t01.mp3`, e senza questa
/// distinzione un indirizzo senza percorso produrrebbe l'estensione `org` — che
/// come contenitore audio non esiste.
///
/// # Perché sta nel dominio
///
/// Perché la chiamano tre posti che non si vedono fra loro: la finestra, quando
/// apre un brano di catalogo in streaming; il prelievo dei cataloghi, quando
/// deve dare un nome al file che sta scaricando e il catalogo non ha dichiarato
/// il formato; e la riproduzione, per il suggerimento che passa al
/// decodificatore. Stanno in tre casse diverse — `aether-desktop`,
/// `aether-catalogo`, `aether-app` — e `aether-catalogo` non dipende, e non deve
/// dipendere, da `aether-app`.
///
/// Finché la regola era scritta due volte, le due scritture erano già divergenti
/// in due dettagli: come si toglieva lo schema, e come si prendeva l'ultimo
/// segmento. È una regola da stringa a stringa, senza niente da aprire: il posto
/// dove non può divergere è questo.
///
/// # Perché la query si butta prima di guardare il punto
///
/// Perché su Audius la query **mente, apposta**: `/v1/tracks/<id>/stream?ext=wav`
/// dice l'estensione del file che l'artista ha caricato, e serve al prelievo per
/// non chiamare `.mp3` un wav. Ma il punto di ascolto restituisce un mp3
/// transcodificato, non quel wav: passare `wav` a symphonia come suggerimento
/// vuol dire farle provare per primo il lettore sbagliato. Un suggerimento
/// assente è meglio di uno falso — symphonia riconosce comunque il contenitore
/// dai marcatori, ed è la strada che prende ogni volta che l'estensione non c'è.
///
/// # L'unico caso in cui non risponde come la copia del prelievo
///
/// Lo schema si toglie a qualunque `://`, mentre la copia che stava in
/// `aether_catalogo::prelievo` toglieva solo `https://` e `http://`: per un
/// indirizzo con uno schema diverso **e senza percorso** — `ftp://a.b` — quella
/// leggeva `b` e questa risponde `None`. Nessuno dei tre chiamanti ci arriva:
/// gli indirizzi del prelievo escono dalle risposte JSON dei cataloghi, che
/// sono API HTTP e restituiscono indirizzi HTTP, e gli altri due suonano da un
/// flusso HTTP già aperto. Fra i due comportamenti resta quello giusto: `b` è
/// l'ultimo pezzo di un nome di host, cioè la stessa cosa che questa funzione
/// esiste per non confondere con un'estensione.
#[must_use]
pub fn estensione_da_url(url: &str) -> Option<String> {
    let senza_query = url.split(['?', '#']).next().unwrap_or(url);
    let dopo_schema = senza_query
        .split_once("://")
        .map_or(senza_query, |(_, resto)| resto);
    // Il nome del file sta nel **percorso**, e un URL senza percorso non ne ha
    // nessuno: senza questo `?` l'host intero finirebbe sotto il punto.
    let (_, percorso) = dopo_schema.split_once('/')?;
    let ultimo = percorso.rsplit('/').next().unwrap_or(percorso);
    let (_, ext) = ultimo.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il suggerimento di formato viene dal percorso, mai dalla query.
    ///
    /// Il caso che conta è il terzo: su Audius `?ext=wav` dice il formato del
    /// file **originale**, mentre dal punto di ascolto arriva un mp3
    /// transcodificato. Un suggerimento falso è peggio di nessun suggerimento.
    #[test]
    fn il_formato_si_indovina_dal_percorso_e_non_dalla_query() {
        assert_eq!(
            estensione_da_url("https://archive.org/download/gd77/t01.FLAC").as_deref(),
            Some("flac")
        );
        assert_eq!(
            estensione_da_url("https://archive.org/download/gd77/t01.mp3?x=1").as_deref(),
            Some("mp3")
        );
        assert_eq!(
            estensione_da_url("https://nodo.esempio/v1/tracks/aB3/stream?ext=wav"),
            None
        );
        assert_eq!(estensione_da_url("https://archive.org/download/gd77"), None);
        // Un «punto» che è in mezzo al dominio e non nel nome del file.
        assert_eq!(estensione_da_url("https://archive.org"), None);
    }

    /// I casi che copriva la copia del prelievo, quando ce n'erano due.
    ///
    /// Il prelievo ci arriva quando il catalogo non dichiara il formato: da
    /// quel che legge qui dipende il nome del file che finisce sul disco, e
    /// un'estensione presa dall'host darebbe un `t01.org` che nessun lettore
    /// aprirebbe.
    #[test]
    fn quel_che_provava_la_copia_del_prelievo() {
        assert_eq!(
            estensione_da_url("https://archive.org/download/x/t01.flac"),
            Some("flac".to_owned())
        );
        assert_eq!(
            estensione_da_url("https://a/b/t01.mp3?token=1"),
            Some("mp3".to_owned())
        );
        assert_eq!(estensione_da_url("https://a/b/senza-estensione"), None);
        // Un dominio non è un'estensione: `archive.org` finisce con un punto e
        // tre lettere esattamente come `t01.mp3`.
        assert_eq!(estensione_da_url("https://archive.org"), None);
        assert_eq!(estensione_da_url("https://archive.org/"), None);
    }

    #[test]
    fn un_host_senza_percorso_non_ha_estensione() {
        // La forma più corta del difetto: niente barra dopo l'host, quindi
        // niente nome di file da cui leggere qualcosa.
        assert_eq!(estensione_da_url("https://archive.org"), None);
        assert_eq!(estensione_da_url("http://a.b.mp3"), None);
    }

    #[test]
    fn con_la_query_l_estensione_resta_quella_del_percorso() {
        assert_eq!(
            estensione_da_url("https://nodo.esempio/audio/t01.mp3?ext=wav").as_deref(),
            Some("mp3"),
            "la query di Audius dice il formato dell'originale, non quello che arriva"
        );
    }
}
