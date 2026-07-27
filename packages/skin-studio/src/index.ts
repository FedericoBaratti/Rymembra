/**
 * @aether/skin-studio — l'editor di skin.
 *
 * L'editor si monta come rotta `/studio` dentro l'app desktop, così l'anteprima è
 * l'app vera e non una replica che può divergere.
 *
 * Questo indice espone il cervello, non l'interfaccia:
 *
 *   draft.ts     il modello di modifica, sulla forma sorgente
 *   contrast.ts  la verifica WCAG, di serie e non a richiesta
 *
 * Sono puri e provati: l'interfaccia che li userà può cambiare forma dieci volte
 * senza che le decisioni che contano vengano rimesse in discussione.
 */

export {
  AA_LARGE,
  AA_NORMAL,
  AAA_NORMAL,
  auditContrast,
  levelFor,
  minimumAlphaFor,
  type ContrastLevel,
  type ContrastPair,
  type ContrastReport
} from './contrast'

export {
  createDraft,
  evaluate,
  forkSkin,
  panelFor,
  type Draft,
  type DraftState,
  type SkinSource
} from './draft'
