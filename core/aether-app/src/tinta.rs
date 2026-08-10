//! La tinta dominante di una copertina.
//!
//! # Perché qui e non nella finestra
//!
//! La strada ovvia sarebbe disegnare la copertina su una `<canvas>` e leggerne
//! i pixel: l'immagine è già caricata, il codice è corto. Ha due prezzi che non
//! valgono lo sconto.
//!
//! Il primo è che una canvas che ha dentro un'immagine di un'altra origine si
//! **contamina**, e `getImageData` smette di rispondere. Per sbloccarla si
//! dovrebbe mettere `Access-Control-Allow-Origin` sul protocollo
//! `aether-cover`, cioè allargare un protocollo che è scritto apposta stretto —
//! il suo unico argomento è un'impronta esadecimale proprio per non essere una
//! lettura di file arbitraria.
//!
//! Il secondo è che venticinquemila pixel si guardano sul filo che disegna
//! l'interfaccia, e li si guarda a ogni cambio di brano.
//!
//! Qui invece la miniatura è già sul disco, è già un JPEG da 160 pixel di lato,
//! e leggerla costa una decodifica che sta sotto il millisecondo.
//!
//! # Come si sceglie la tinta
//!
//! Non è il colore medio: la media di una copertina è quasi sempre un marrone
//! grigio, perché mediare tinte opposte le annulla. Si contano invece le
//! tonalità, in trentasei sacche da dieci gradi, scartando prima quel che non
//! porta tonalità:
//!
//! - il quasi nero e il quasi bianco, che sono il fondo e le luci;
//! - i grigi, che una tonalità ce l'hanno solo per arrotondamento.
//!
//! Vince la sacca più popolosa pesata per quanto è satura — così un cielo
//! azzurro slavato che occupa metà copertina non batte il rosso acceso del
//! soggetto —, e la tinta è la media **circolare** di quella sacca. Circolare
//! perché 359° e 1° distano due gradi, e una media aritmetica direbbe 180: il
//! ciano esatto complementare del rosso che stiamo misurando.
//!
//! Una copertina in bianco e nero non ha una tinta, e la risposta è `None`.
//! Inventargliene una vorrebbe dire dipingere l'interfaccia con il rumore di
//! compressione del JPEG.

/// Quante sacche di tonalità.
const SACCHE: usize = 36;

/// Quanti gradi per sacca. Le due costanti devono coprire il giro.
const GRADI_PER_SACCA: f64 = 10.0;
const _: () = assert!(SACCHE * 10 == 360, "le sacche devono coprire il giro");

/// Sotto questa luminosità un pixel è nero: nessuna tonalità sopravvive alla
/// compressione, e il fondo di una copertina scura sarebbe la maggioranza.
const NERO: f64 = 0.15;

/// Sopra, è una luce. Il bianco bruciato non ha tonalità, ne ha il residuo.
const BIANCO: f64 = 0.95;

/// Sotto questa saturazione è un grigio.
const GRIGIO: f64 = 0.20;

/// Quanta parte della copertina deve portare una tonalità perché ce ne sia una.
///
/// Un quinto: sotto, si sta guardando il logo dell'etichetta in un angolo, non
/// il colore del disco.
const QUOTA_MINIMA: f64 = 0.05;

/// La saturazione che si restituisce, al massimo.
///
/// La sacca vincente può avere una media satura al 100%, che ricostruita in RGB
/// dà un colore da schermo di prova. Il taglio del contrasto a valle riduce
/// comunque il croma, ma partire da un colore già ragionevole vuol dire
/// spostarlo di meno.
const TETTO_SATURAZIONE: f64 = 0.85;

/// Quel che si accumula per ogni sacca di tonalità.
#[derive(Debug, Default, Clone, Copy)]
struct Sacca {
    /// Quanti pixel ci sono caduti.
    quanti: u32,
    /// Le componenti della media circolare della tonalità.
    seno: f64,
    coseno: f64,
    /// La somma delle saturazioni e delle luminosità, per farne la media.
    saturazione: f64,
    valore: f64,
}

/// Un pixel in tonalità, saturazione, valore.
///
/// HSV e non OKLCH: qui si sta **classificando**, non misurando. Serve sapere
/// se un pixel è grigio, se è nero, e in che fetta di ruota cade — e per queste
/// tre domande HSV costa tre confronti mentre OKLCH costa tre radici cubiche,
/// venticinquemila volte. Il colore che esce di qui viene poi rimisurato in
/// OKLCH da `aether-skin`, che è dove la precisione percettiva conta davvero.
fn hsv(rosso: u8, verde: u8, blu: u8) -> (f64, f64, f64) {
    let r = f64::from(rosso) / 255.0;
    let g = f64::from(verde) / 255.0;
    let b = f64::from(blu) / 255.0;
    let massimo = r.max(g).max(b);
    let minimo = r.min(g).min(b);
    let ampiezza = massimo - minimo;

    if ampiezza <= 0.0 {
        return (0.0, 0.0, massimo);
    }
    let gradi = if massimo <= r {
        60.0 * (((g - b) / ampiezza) % 6.0)
    } else if massimo <= g {
        60.0 * ((b - r) / ampiezza + 2.0)
    } else {
        60.0 * ((r - g) / ampiezza + 4.0)
    };
    (
        if gradi < 0.0 { gradi + 360.0 } else { gradi },
        ampiezza / massimo,
        massimo,
    )
}

/// Da tonalità, saturazione e valore ai tre canali.
fn da_hsv(gradi: f64, saturazione: f64, valore: f64) -> [u8; 3] {
    let cima = valore * saturazione;
    let fetta = gradi.rem_euclid(360.0) / 60.0;
    let lato = cima * (1.0 - ((fetta % 2.0) - 1.0).abs());
    let base = valore - cima;

    // A scale invece che con un indice: `fetta as usize` sarebbe un troncamento
    // da giustificare per risparmiare cinque confronti.
    let (r, g, b) = if fetta < 1.0 {
        (cima, lato, 0.0)
    } else if fetta < 2.0 {
        (lato, cima, 0.0)
    } else if fetta < 3.0 {
        (0.0, cima, lato)
    } else if fetta < 4.0 {
        (0.0, lato, cima)
    } else if fetta < 5.0 {
        (lato, 0.0, cima)
    } else {
        (cima, 0.0, lato)
    };
    [byte(r + base), byte(g + base), byte(b + base)]
}

/// Un canale da un numero in 0..=1.
fn byte(valore: f64) -> u8 {
    let scalato = (valore.clamp(0.0, 1.0) * 255.0).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "il valore è già bloccato in 0..=255 e arrotondato dalla riga sopra"
    )]
    let intero = scalato as u8;
    intero
}

/// In quale sacca cade una tonalità.
fn sacca_di(gradi: f64) -> usize {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "la tonalità è in 0..360 e il quoziente sta in 0..36, poi limitato"
    )]
    let indice = (gradi / GRADI_PER_SACCA) as usize;
    indice.min(SACCHE - 1)
}

/// La tinta dominante di un'immagine, o niente se non ne ha una.
///
/// Il risultato è un colore pieno in sRGB. **Non è ancora un accento**: prima
/// di finire sulla finestra deve passare da `aether_skin::accento_sicuro`, che
/// è l'unico posto in cui si decide se un colore è leggibile. Questa funzione
/// dice soltanto *di che colore è il disco*.
#[must_use]
pub fn dominante(immagine: &image::RgbImage) -> Option<[u8; 3]> {
    let totale = immagine.width().saturating_mul(immagine.height());
    if totale == 0 {
        return None;
    }

    let mut sacche = [Sacca::default(); SACCHE];
    let mut colorati: u32 = 0;

    for pixel in immagine.pixels() {
        let image::Rgb([r, g, b]) = *pixel;
        let (gradi, saturazione, valore) = hsv(r, g, b);
        if !(NERO..=BIANCO).contains(&valore) || saturazione < GRIGIO {
            continue;
        }
        colorati = colorati.saturating_add(1);
        let Some(sacca) = sacche.get_mut(sacca_di(gradi)) else {
            continue;
        };
        let radianti = gradi.to_radians();
        sacca.quanti = sacca.quanti.saturating_add(1);
        sacca.seno += radianti.sin();
        sacca.coseno += radianti.cos();
        sacca.saturazione += saturazione;
        sacca.valore += valore;
    }

    if f64::from(colorati) < f64::from(totale) * QUOTA_MINIMA {
        return None;
    }

    // Il punteggio è popolazione per saturazione media, cioè la somma delle
    // saturazioni. Scritto così invece che come prodotto di due medie perché è
    // lo stesso numero senza una divisione in mezzo, e perché rende ovvio che
    // una sacca larga e slavata e una stretta e accesa possono pareggiare.
    let migliore = sacche
        .iter()
        .filter(|sacca| sacca.quanti > 0)
        .max_by(|uno, due| uno.saturazione.total_cmp(&due.saturazione))?;

    let quanti = f64::from(migliore.quanti);
    let gradi = migliore
        .seno
        .atan2(migliore.coseno)
        .to_degrees()
        .rem_euclid(360.0);
    Some(da_hsv(
        gradi,
        (migliore.saturazione / quanti).min(TETTO_SATURAZIONE),
        migliore.valore / quanti,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un'immagine piena di un colore solo.
    fn tinta_unica(r: u8, g: u8, b: u8) -> image::RgbImage {
        image::RgbImage::from_pixel(40, 40, image::Rgb([r, g, b]))
    }

    /// Un fondo con dentro un rettangolo di un altro colore.
    fn con_soggetto(
        fondo: [u8; 3],
        soggetto: [u8; 3],
        larghezza_soggetto: u32,
    ) -> image::RgbImage {
        let mut buffer = image::RgbImage::from_pixel(100, 100, image::Rgb(fondo));
        for (x, _y, pixel) in buffer.enumerate_pixels_mut() {
            if x < larghezza_soggetto {
                *pixel = image::Rgb(soggetto);
            }
        }
        buffer
    }

    /// La distanza fra due tonalità sulla ruota.
    fn scarto(uno: f64, due: f64) -> f64 {
        let grezzo = (uno - due).abs() % 360.0;
        grezzo.min(360.0 - grezzo)
    }

    fn tonalita(colore: [u8; 3]) -> f64 {
        let [r, g, b] = colore;
        hsv(r, g, b).0
    }

    #[test]
    fn un_colore_solo_torna_indietro_uguale() {
        let arancio = [230, 120, 30];
        let trovata = dominante(&tinta_unica(230, 120, 30)).expect("una tinta c'è");
        assert!(
            scarto(tonalita(trovata), tonalita(arancio)) < 2.0,
            "{trovata:?} contro {arancio:?}"
        );
    }

    #[test]
    fn il_bianco_e_nero_non_ha_una_tinta() {
        // È il caso che conta: senza questo, il rumore del JPEG diventerebbe
        // l'accento dell'interfaccia.
        assert!(dominante(&tinta_unica(200, 200, 200)).is_none());
        assert!(dominante(&tinta_unica(0, 0, 0)).is_none());
        assert!(dominante(&tinta_unica(255, 255, 255)).is_none());
        // Un grigio appena tiepido resta un grigio.
        assert!(dominante(&tinta_unica(130, 128, 126)).is_none());
    }

    #[test]
    fn un_soggetto_acceso_batte_un_fondo_slavato() {
        // Sessanta per cento di azzurro slavato contro quaranta di rosso pieno:
        // il colore medio direbbe rosa sporco, il conteggio dice rosso.
        let immagine = con_soggetto([150, 175, 200], [220, 30, 40], 40);
        let trovata = dominante(&immagine).expect("una tinta c'è");
        assert!(
            scarto(tonalita(trovata), tonalita([220, 30, 40])) < 12.0,
            "doveva vincere il rosso, ha vinto {trovata:?}"
        );
    }

    #[test]
    fn il_fondo_nero_non_conta_come_colore() {
        // Una copertina nera con una scritta colorata: il nero è la maggioranza
        // schiacciante e non deve poter dire niente sulla tonalità.
        let immagine = con_soggetto([4, 4, 6], [40, 200, 120], 20);
        let trovata = dominante(&immagine).expect("il verde c'è");
        assert!(
            scarto(tonalita(trovata), tonalita([40, 200, 120])) < 12.0,
            "{trovata:?}"
        );
    }

    #[test]
    fn una_traccia_di_colore_non_basta() {
        // Il due per cento della copertina è colorato: è il bollino dell'etichetta,
        // non la tinta del disco.
        let immagine = con_soggetto([20, 20, 20], [220, 30, 40], 2);
        assert!(dominante(&immagine).is_none());
    }

    #[test]
    fn le_tonalita_opposte_non_si_annullano() {
        // Rosso a 355° e rosso a 5°: la media aritmetica darebbe 180°, cioè
        // ciano — il complementare esatto di quel che c'è nell'immagine.
        let mut buffer = image::RgbImage::new(100, 100);
        for (x, _y, pixel) in buffer.enumerate_pixels_mut() {
            *pixel = if x < 50 {
                image::Rgb(da_hsv(355.0, 0.8, 0.8))
            } else {
                image::Rgb(da_hsv(5.0, 0.8, 0.8))
            };
        }
        let trovata = dominante(&buffer).expect("una tinta c'è");
        let gradi = tonalita(trovata);
        assert!(
            scarto(gradi, 0.0) < 15.0,
            "doveva restare sul rosso, è finita a {gradi}°"
        );
    }

    #[test]
    fn il_giro_di_hsv_torna_al_punto_di_partenza() {
        for campione in [
            [230, 120, 30],
            [40, 200, 120],
            [20, 30, 200],
            [255, 0, 0],
            [0, 0, 0],
            [255, 255, 255],
            [17, 200, 199],
        ] {
            let [r, g, b] = campione;
            let (h, s, v) = hsv(r, g, b);
            assert_eq!(da_hsv(h, s, v), campione, "andata e ritorno su {campione:?}");
        }
    }

    #[test]
    fn un_immagine_vuota_non_fa_cadere_niente() {
        assert!(dominante(&image::RgbImage::new(0, 0)).is_none());
    }
}
