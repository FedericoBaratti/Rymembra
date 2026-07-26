import { getDb } from '../db'
import { createApiCache, type ApiCache } from './apiCache'

// Lives apart from apiCache.ts so unit tests can import createApiCache
// without dragging in electron via db.ts.

let singleton: ApiCache | null = null

export function getApiCache(): ApiCache {
  if (!singleton) singleton = createApiCache(getDb())
  return singleton
}
