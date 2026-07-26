/**
 * Diacritic-insensitive text folding, shared by the renderer and backend.
 *
 * Strips combining marks (U+0300–U+036F) after NFD normalization and lowercases.
 * Uses a codepoint filter rather than a regex literal so the source stays plain
 * ASCII (kept in sync with the porting build, which must run on nodejs-mobile's
 * Node 12 without unicode-property regex).
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
