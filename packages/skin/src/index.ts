/**
 * Punto d'ingresso pubblico di @aether/skin.
 *
 * Nella Fase 3 qui arrivano: lo schema zod di skin.json, la libreria di effetti
 * parametrici, il compilatore verso CSS, il parts registry e la lettura/scrittura
 * del pacchetto .aeskin.
 *
 * Il pacchetto è isomorfo: gira nel renderer (compilazione e applicazione) e nel
 * backend (validazione all'installazione). L'unico modulo che tocca il DOM è
 * `apply/`, e i suoi test si chiamano *.dom.test.ts.
 */
export const SKIN_FORMAT_VERSION = 1
