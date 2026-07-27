/**
 * @aether/skin — il formato skin, il suo compilatore e il suo contratto.
 *
 * La forma del pacchetto segue una decisione sola, presa all'inizio: **una skin è
 * dati, non CSS.** Da lì viene tutto il resto.
 *
 *   values.ts    i valori ammessi, e il fatto che nessuno passi mai come testo
 *   tokens.ts    il registro: il contratto fra skin e componenti, come dati
 *   effects.ts   gli effetti parametrici, con il loro costo dichiarato
 *   schema.ts    la forma di skin.json
 *   parse.ts     l'ingresso, e messaggi che dicono cosa rifiutare
 *   compile.ts   l'unico autore di CSS del sistema
 *
 * Il pacchetto è isomorfo: la validazione gira anche sul backend
 * all'installazione, la compilazione nel renderer, dove il foglio si sostituisce
 * con `adoptedStyleSheets`.
 */

export {
  EASING_KEYWORDS,
  LENGTH_UNITS,
  MAX_DURATION_MS,
  SYSTEM_FALLBACKS,
  ZERO_LENGTH,
  clampLengthSchema,
  colorSchema,
  contrastRatio,
  durationSchema,
  formatLengthValue,
  isClampLength,
  lengthValueSchema,
  easingSchema,
  fontFamilySchema,
  fontStackSchema,
  formatColor,
  formatDuration,
  formatEasing,
  formatFontStack,
  formatLength,
  formatRgbTriple,
  lengthSchema,
  parseColor,
  parseDuration,
  parseLength,
  relativeLuminance,
  unitlessSchema,
  withAlpha,
  type ClampLength,
  type Duration,
  type Easing,
  type EasingKeyword,
  type Length,
  type LengthUnit,
  type LengthValue,
  type Rgba
} from './values'

export {
  BUILTIN_SKIN_IDS,
  BUILTIN_SKIN_SOURCES,
  NOTHING_SKIN_SOURCE,
  PLAIN_SKIN_SOURCE
} from './builtin'

export {
  PARTS,
  PART_NAMES,
  PART_STATES,
  compilePart,
  isPartName,
  part,
  partDef,
  parts,
  partStyleSchema,
  partsInGroup,
  skinPartsSchema,
  type CompiledPart,
  type PartDef,
  type PartGroup,
  type PartName,
  type PartState,
  type PartStyle
} from './parts'

export {
  DYNAMIC_SOURCES,
  REQUIRED_TOKEN_IDS,
  TOKENS,
  TOKEN_IDS,
  buildTokensSchema,
  colorValueSchema,
  dynamicSourceSchema,
  isTokenId,
  schemaForKind,
  shadowLayerSchema,
  shadowValueSchema,
  tokenDef,
  tokenRefSchema,
  tokensInGroup,
  type ColorValue,
  type DynamicSource,
  type ShadowValue,
  type TokenDef,
  type TokenGroup,
  type TokenId,
  type TokenKind
} from './tokens'

export {
  COST_CLASSES,
  COST_WEIGHT,
  EFFECT_COST,
  EFFECT_TARGET,
  SURFACE_COST_BUDGET,
  effectCost,
  effectSchema,
  exceedsBudget,
  stackCost,
  type CostClass,
  type Effect,
  type EffectName
} from './effects'

export {
  MOTION_INTENSITIES,
  SKIN_FORMAT_VERSION,
  routeTransitionSchema,
  skinCapabilitiesSchema,
  skinDocumentSchema,
  skinIdSchema,
  skinLayoutSchema,
  skinMetaSchema,
  skinMotionSchema,
  skinPatternsSchema,
  type MotionIntensity,
  type SkinDocument,
  type SkinTokens
} from './schema'

export {
  checkSkin,
  declaredTokens,
  parseSkin,
  type SkinIssue,
  type SkinWarning
} from './parse'

export { compileEffect, compileSkin, type CompiledSkin } from './compile'

export {
  MANIFEST_NAME,
  PACKAGE_LIMITS,
  PREVIEW_NAME,
  packageFileName,
  readSkinPackage,
  writeSkinPackage,
  type SkinAsset,
  type SkinPackage,
  type WritePackageInput
} from './package'
