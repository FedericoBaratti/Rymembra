// sql.js ships no type declarations; the shim casts it to the needed shape.
declare module 'sql.js' {
  const initSqlJs: unknown
  export default initSqlJs
}
