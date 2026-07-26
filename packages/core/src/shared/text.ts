/**
 * Diacritic-insensitive text folding, shared by the renderer (galaxy/search
 * filtering) and the backend (the SQL `afold()` function used by search()).
 *
 * Strips combining marks (U+0300–U+036F) after NFD normalization and lowercases.
 * Uses a codepoint filter rather than a regex literal so the source stays plain
 * ASCII and runs on nodejs-mobile's Node 12 without unicode-property regex.
 */
export function foldText(input: string): string {
  const n = input.normalize('NFD')
  let out = ''
  for (let i = 0; i < n.length; i++) {
    const c = n.charCodeAt(i)
    if (c >= 0x0300 && c <= 0x036f) continue
    out += n[i]
  }
  return out.toLowerCase()
}
