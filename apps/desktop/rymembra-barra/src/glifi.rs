//! I quattro simboli dei pulsanti, disegnati qui.
//!
//! # Perché disegnati e non presi da un file
//!
//! Perché la misura giusta non si sa prima: l'anteprima vuole icone del lato
//! delle «icone piccole» del sistema, che a 100% è sedici pixel, a 125% venti,
//! a 150% ventiquattro, e così via. Un file `.ico` dovrebbe portarsele tutte, e
//! a una scala che non ha previsto Windows ne stirerebbe una. Disegnando si
//! produce esattamente il lato che serve, e il colore che serve — chiaro sulla
//! barra scura, scuro sulla barra chiara — senza otto file per quattro simboli.
//!
//! Le forme sono quelle che chiunque riconosce: un triangolo, due barre, un
//! triangolo con una sbarra. Si descrivono in coordinate da zero a uno e si
//! campionano sedici volte per pixel, così i bordi obliqui sfumano invece di
//! fare la scaletta.

/// Uno dei quattro simboli.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glifo {
    /// Sbarra e triangolo verso sinistra.
    Precedente,
    /// Triangolo verso destra.
    Riproduci,
    /// Due barre.
    Pausa,
    /// Triangolo verso destra e sbarra.
    Successivo,
}

/// Quanti campioni per lato di pixel: sedici per pixel in tutto.
///
/// Abbastanza perché un bordo obliquo abbia diciassette gradini di
/// trasparenza, che a sedici pixel di lato non si distinguono da una sfumatura
/// vera; pochi abbastanza perché un'icona da trentadue pixel costi sedicimila
/// prove di appartenenza, cioè niente.
const CAMPIONI: u32 = 4;

/// Una forma piena, in coordinate da zero a uno con l'origine in alto a
/// sinistra.
#[derive(Debug, Clone, Copy)]
enum Forma {
    /// Un rettangolo: sinistra, alto, destra, basso.
    Rettangolo(f64, f64, f64, f64),
    /// Un triangolo: tre vertici.
    Triangolo((f64, f64), (f64, f64), (f64, f64)),
}

impl Forma {
    /// Il punto sta dentro la forma.
    fn contiene(self, x: f64, y: f64) -> bool {
        match self {
            Self::Rettangolo(sinistra, alto, destra, basso) => {
                x >= sinistra && x <= destra && y >= alto && y <= basso
            }
            Self::Triangolo(a, b, c) => {
                // Il punto sta dalla stessa parte di tutti e tre i lati: i tre
                // prodotti vettoriali hanno lo stesso segno, qualunque sia il
                // verso in cui i vertici sono scritti.
                let lato = |p: (f64, f64), q: (f64, f64)| {
                    (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0)
                };
                let (d1, d2, d3) = (lato(a, b), lato(b, c), lato(c, a));
                let qualche_negativo = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
                let qualche_positivo = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
                !(qualche_negativo && qualche_positivo)
            }
        }
    }
}

/// Le forme di un simbolo.
///
/// Il triangolo di «riproduci» è spostato un poco a destra del centro: il suo
/// baricentro sta a un terzo della base, e centrato sul riquadro sembrerebbe
/// spinto a sinistra. «Precedente» è lo specchio esatto di «successivo»: la
/// prova lo controlla.
fn forme(glifo: Glifo) -> &'static [Forma] {
    const PRECEDENTE: [Forma; 2] = [
        Forma::Rettangolo(0.24, 0.22, 0.34, 0.78),
        Forma::Triangolo((0.76, 0.22), (0.76, 0.78), (0.36, 0.5)),
    ];
    const RIPRODUCI: [Forma; 1] = [Forma::Triangolo((0.3, 0.2), (0.3, 0.8), (0.8, 0.5))];
    const PAUSA: [Forma; 2] = [
        Forma::Rettangolo(0.27, 0.22, 0.42, 0.78),
        Forma::Rettangolo(0.58, 0.22, 0.73, 0.78),
    ];
    const SUCCESSIVO: [Forma; 2] = [
        Forma::Triangolo((0.24, 0.22), (0.24, 0.78), (0.64, 0.5)),
        Forma::Rettangolo(0.66, 0.22, 0.76, 0.78),
    ];
    match glifo {
        Glifo::Precedente => &PRECEDENTE,
        Glifo::Riproduci => &RIPRODUCI,
        Glifo::Pausa => &PAUSA,
        Glifo::Successivo => &SUCCESSIVO,
    }
}

/// Disegna il simbolo in un quadrato di `lato` pixel, nel `colore` dato.
///
/// Restituisce i pixel riga per riga dall'alto, quattro byte per pixel
/// nell'ordine che Windows vuole per un'icona a 32 bit: **blu, verde, rosso,
/// alfa**. Il colore è lo stesso per ogni pixel e la forma sta tutta
/// nell'alfa, non premoltiplicato — come in un file `.ico`.
///
/// `colore` è in rosso, verde, blu.
#[expect(
    clippy::integer_division,
    reason = "l'alfa di un pixel in diciassette gradini: il resto della divisione \
              è meno di un sedicesimo di gradino, e l'arrotondamento lo assorbe"
)]
pub(crate) fn disegna(glifo: Glifo, lato: u32, colore: [u8; 3]) -> Vec<u8> {
    let [rosso, verde, blu] = colore;
    let forme = forme(glifo);
    let misura = f64::from(lato.max(1));
    let campioni = CAMPIONI * CAMPIONI;
    let pixel = usize::try_from(lato).unwrap_or(0);
    let mut uscita = Vec::with_capacity(pixel * pixel * 4);
    for riga in 0..lato {
        for colonna in 0..lato {
            let mut dentro = 0u32;
            for sy in 0..CAMPIONI {
                for sx in 0..CAMPIONI {
                    // Al centro di ogni sottocella: così il campionamento è
                    // simmetrico, e lo specchio di un simbolo dà lo specchio
                    // dei suoi pixel.
                    let x =
                        (f64::from(colonna) + (f64::from(sx) + 0.5) / f64::from(CAMPIONI)) / misura;
                    let y =
                        (f64::from(riga) + (f64::from(sy) + 0.5) / f64::from(CAMPIONI)) / misura;
                    if forme.iter().any(|forma| forma.contiene(x, y)) {
                        dentro += 1;
                    }
                }
            }
            let alfa = u8::try_from((dentro * 255 + campioni / 2) / campioni).unwrap_or(u8::MAX);
            uscita.extend_from_slice(&[blu, verde, rosso, alfa]);
        }
    }
    uscita
}

#[cfg(test)]
mod prove {
    use super::*;

    const BIANCO: [u8; 3] = [0xFF, 0xFF, 0xFF];

    /// L'alfa del pixel in colonna `x` e riga `y`.
    fn alfa(pixel: &[u8], lato: u32, x: u32, y: u32) -> u8 {
        let posto = usize::try_from((y * lato + x) * 4 + 3).unwrap_or(usize::MAX);
        pixel.get(posto).copied().unwrap_or_default()
    }

    #[test]
    fn ogni_simbolo_riempie_esattamente_il_quadrato_chiesto() {
        for lato in [16, 20, 24, 32, 48] {
            for glifo in [
                Glifo::Precedente,
                Glifo::Riproduci,
                Glifo::Pausa,
                Glifo::Successivo,
            ] {
                let pixel = disegna(glifo, lato, BIANCO);
                assert_eq!(pixel.len(), usize::try_from(lato * lato * 4).unwrap_or(0));
            }
        }
    }

    #[test]
    fn gli_angoli_sono_trasparenti_e_il_centro_di_riproduci_e_pieno() {
        let lato = 20;
        let pixel = disegna(Glifo::Riproduci, lato, BIANCO);
        for (x, y) in [(0, 0), (lato - 1, 0), (0, lato - 1), (lato - 1, lato - 1)] {
            assert_eq!(alfa(&pixel, lato, x, y), 0, "angolo ({x}, {y})");
        }
        assert_eq!(alfa(&pixel, lato, 10, 10), 255);
    }

    #[test]
    fn fra_le_due_barre_della_pausa_c_e_il_vuoto() {
        let lato = 32;
        let pixel = disegna(Glifo::Pausa, lato, BIANCO);
        assert_eq!(alfa(&pixel, lato, 16, 16), 0);
        assert_eq!(alfa(&pixel, lato, 11, 16), 255, "barra sinistra");
        assert_eq!(alfa(&pixel, lato, 20, 16), 255, "barra destra");
    }

    #[test]
    fn i_bordi_obliqui_sfumano() {
        // Almeno un pixel a metà: senza il sovracampionamento il triangolo
        // avrebbe solo pixel pieni e vuoti, cioè la scaletta.
        let lato = 16;
        let pixel = disegna(Glifo::Riproduci, lato, BIANCO);
        let sfumati = pixel
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p.get(3).is_some_and(|&a| a > 0 && a < 255))
            .count();
        assert!(sfumati > 0);
    }

    #[test]
    fn precedente_e_lo_specchio_di_successivo() {
        let lato = 24;
        let prima = disegna(Glifo::Precedente, lato, BIANCO);
        let dopo = disegna(Glifo::Successivo, lato, BIANCO);
        for y in 0..lato {
            for x in 0..lato {
                let a = i32::from(alfa(&prima, lato, x, y));
                let b = i32::from(alfa(&dopo, lato, lato - 1 - x, y));
                // Un sedicesimo di tolleranza: un campione che cade
                // esattamente su un lato può finire dentro da una parte e
                // fuori dall'altra per un arrotondamento.
                assert!((a - b).abs() <= 16, "({x}, {y}): {a} contro {b}");
            }
        }
    }

    #[test]
    fn il_colore_e_quello_chiesto_in_ordine_blu_verde_rosso() {
        let pixel = disegna(Glifo::Pausa, 16, [0x10, 0x20, 0x30]);
        for p in pixel.as_chunks::<4>().0 {
            assert_eq!(p.get(..3), Some([0x30, 0x20, 0x10].as_slice()));
        }
    }
}
