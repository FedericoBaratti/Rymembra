/**
 * La camera della scena dello spettro: dove sta l'occhio e dove guarda, a ogni
 * fotogramma.
 *
 * Un modulo a parte perché è puro e si prova con `node --test`
 * (`test/cameraSpettro.test.ts`): `Spettro3D` porta con sé WebGL e l'IPC.
 */

/**
 * Le manopole della skin che muovono la camera: vedi `rileggi` in
 * `spettro3d/risorse.ts`.
 */
export interface ManopoleCamera {
  /** `canvas.viz.sway`: quanto la stanza gira da sola. */
  oscillazione: number;
  /** `canvas.viz.beat`: quanto la spinge la musica. */
  pulsazione: number;
  /** `canvas.viz.eye`: l'altezza dell'occhio a riposo. */
  occhio: number;
}

/** I sei numeri che `camera()` vuole: l'occhio e il punto guardato. */
export interface PosaCamera {
  occhioX: number;
  occhioY: number;
  occhioZ: number;
  bersaglioX: number;
  bersaglioY: number;
  bersaglioZ: number;
}

/**
 * Quanto tempo di scena è passato dal fotogramma di prima, in millisecondi.
 *
 * Il primo fotogramma non ha un prima, e un risveglio lascia un buco lungo
 * quanto vuole: da lì il minimo (un fotogramma a sessanta hertz) e il tetto
 * (un decimo di secondo).
 */
export function passoTempo(adesso: number, precedente: number): number {
  return precedente > 0 ? Math.min(adesso - precedente, 100) : 16.7;
}

/**
 * Dove sta la camera.
 *
 * `tempoScena` è in secondi, ed è il tempo **della scena**: la somma dei
 * `passoTempo` dei fotogrammi disegnati, non l'orologio da quando la scena è
 * nata. Contato dall'orologio, l'oscillazione correva anche mentre la scena
 * dormiva, e al risveglio dopo una pausa lunga l'occhio ripartiva da una fase
 * qualunque: fino a mezza unità di mondo in un fotogramma, il teletrasporto
 * che `seguito` esiste per togliere al colpo.
 *
 * Con il moto ridotto — del sistema o della skin — la camera sta ferma: niente
 * oscillazione, niente avvicinamento, e niente occhio che sale col colpo. Il
 * terzo mancava, e l'orizzonte pulsava a tempo di grancassa proprio a chi
 * aveva chiesto di togliere il movimento.
 *
 * I numeri sono quelli di sempre: a manopole uno e uno, e col moto pieno, la
 * posa è identica a prima.
 */
export function posaCamera(
  ridotto: boolean,
  seguito: number,
  tempoScena: number,
  m: ManopoleCamera,
): PosaCamera {
  // L'oscillazione: lentissima, e la sua ampiezza cresce col colpo. Vedi il
  // commento accanto alla chiamata in `Spettro3D` per le due manopole.
  const oscilla = ridotto
    ? 0
    : Math.sin(tempoScena * 0.21) * (0.16 * m.oscillazione + seguito * 0.22 * m.pulsazione);
  const avvicina = ridotto ? 0 : seguito * 0.06 * m.pulsazione;
  const sale = ridotto ? 0 : seguito * 0.03 * m.pulsazione;
  return {
    occhioX: Math.sin(oscilla) * 1.5,
    occhioY: m.occhio + sale,
    occhioZ: 1.45 - avvicina,
    bersaglioX: Math.sin(oscilla) * 0.4,
    bersaglioY: 0.02,
    bersaglioZ: -2.2,
  };
}
