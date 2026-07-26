import { create } from 'zustand'
import type { Track, RepeatMode, PersistedQueue } from '@shared/types'
import { playerEngine, type QueueEntry } from '@/lib/player'
import { isMobile } from '@/lib/platform'
import { consumePendingEpisodeSeek, isEpisodeTrack } from '@/lib/podcast'
import { toast } from './useToastStore'
import i18n from '@/i18n'

function shuffledOrder(length: number, firstIndex: number): number[] {
  const rest = Array.from({ length }, (_, i) => i).filter((i) => i !== firstIndex)
  for (let i = rest.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[rest[i], rest[j]] = [rest[j], rest[i]]
  }
  return [firstIndex, ...rest]
}

let sleepTimer: number | null = null
let volumeSaveTimer: number | null = null
let queueSaveTimer: number | null = null
// false until playerEngine has actually loaded a Howl (e.g. after a queue restore)
let engineLoaded = false

// ---- scrobble listen tracking (played_at = listen start, per Last.fm spec) ----
let listen: {
  trackId: number
  startedAtSec: number
  accumulatedSec: number
  playingSince: number | null
} | null = null

function pauseListen(): void {
  if (listen?.playingSince != null) {
    listen.accumulatedSec += (Date.now() - listen.playingSince) / 1000
    listen.playingSince = null
  }
}

function finalizeListen(): void {
  if (!listen) return
  pauseListen()
  const { trackId, accumulatedSec, startedAtSec } = listen
  listen = null
  // skip blips; the 50%/4min rule is enforced in the main process
  if (accumulatedSec >= 5) {
    void window.aether.submitScrobble(trackId, Math.round(accumulatedSec), startedAtSec)
  }
}

function beginListen(track: Track): void {
  if (listen?.trackId === track.id) {
    // resume of the same listen (Howl fires onplay on resume too)
    if (listen.playingSince == null) listen.playingSince = Date.now()
    return
  }
  finalizeListen()
  listen = {
    trackId: track.id,
    startedAtSec: Math.floor(Date.now() / 1000),
    accumulatedSec: 0,
    playingSince: Date.now()
  }
  void window.aether.nowPlaying(track.id)
}

interface PlayerState {
  queue: Track[]
  order: number[]
  orderPos: number
  currentTrack: Track | null
  isPlaying: boolean
  repeat: RepeatMode
  shuffle: boolean
  volume: number
  muted: boolean
  playbackRate: number
  sleepEndsAt: number | null
  loadError: string | null

  playTracks: (tracks: Track[], startIndex?: number) => void
  playQueueIndex: (queueIndex: number) => void
  enqueue: (tracks: Track[]) => void
  playNext: (tracks: Track[]) => void
  removeFromQueue: (queueIndex: number) => void
  reorderQueue: (from: number, to: number) => void
  clearQueue: () => void
  togglePlay: () => void
  next: (manual?: boolean) => void
  previous: () => void
  seekBy: (delta: number) => void
  setVolume: (v: number) => void
  adjustVolume: (delta: number) => void
  toggleMute: () => void
  cycleRepeat: () => void
  toggleShuffle: () => void
  setPlaybackRate: (rate: number) => void
  setSleepTimer: (minutes: number | null) => void
  peekNext: () => Track | null
  updateTrack: (track: Track) => void
  restoreQueue: (tracks: Track[], persisted: PersistedQueue) => void
}

export const usePlayerStore = create<PlayerState>((set, get) => {
  // Podcast episodes ride through the same queue/player as library tracks but use
  // a synthetic negative id (= -episodeId) + a stream_url, so they must skip the
  // library-only side effects (play_count / scrobbling) and instead persist their
  // listen position so the episode can resume.
  const isEpisode = isEpisodeTrack
  const savePodcastProgress = (track: Track | null, played: boolean): void => {
    if (isEpisode(track)) {
      void window.aether.setEpisodeProgress(-(track as Track).id, playerEngine.position(), played)
    }
  }

  // Persist the listen position every ~10s while an episode plays, so the
  // resume point survives an OS kill in the background (onPause/onEnd may never
  // fire then). Only runs for episodes; stopped the moment one isn't playing.
  let podcastAutosave: number | null = null
  const stopPodcastAutosave = (): void => {
    if (podcastAutosave != null) {
      window.clearInterval(podcastAutosave)
      podcastAutosave = null
    }
  }
  const startPodcastAutosave = (): void => {
    stopPodcastAutosave()
    podcastAutosave = window.setInterval(() => {
      const t = get().currentTrack
      if (isEpisode(t) && playerEngine.isPlaying()) savePodcastProgress(t, false)
      else stopPodcastAutosave()
    }, 10_000)
  }

  playerEngine.setCallbacks({
    onEnd: () => {
      stopPodcastAutosave()
      savePodcastProgress(get().currentTrack, true)
      get().next(false)
    },
    onPlay: (track) => {
      set({ isPlaying: true, loadError: null })
      if (isEpisode(track)) {
        // Resume where we left off (only the first time this episode starts).
        const at = consumePendingEpisodeSeek(track.id)
        if (at != null) playerEngine.seek(at)
        startPodcastAutosave()
      } else {
        void window.aether.recordPlay(track.id)
        beginListen(track)
      }
    },
    onPause: () => {
      set({ isPlaying: false })
      stopPodcastAutosave()
      savePodcastProgress(get().currentTrack, false)
      pauseListen()
    },
    onLoadError: (track, message) => {
      stopPodcastAutosave()
      // ExoPlayer has no AIFF decoder: on Android the generic loaderror would be
      // invisible (PlayerBar shows loadError only without a current track), so
      // surface a specific toast instead of failing silently.
      const ext = track.path.split('?')[0].split('.').pop()?.toLowerCase()
      if (isMobile && (ext === 'aif' || ext === 'aiff')) {
        const msg = i18n.t('player.aiff_unsupported_mobile', { title: track.title })
        toast.error(msg)
        set({ loadError: msg, isPlaying: false })
        return
      }
      set({ loadError: `${track.title}: ${message}`, isPlaying: false })
    },
    // Mobile: the native ExoPlayer queue auto-advanced (or was seeked from the
    // notification) to another item — possibly while the WebView was frozen.
    // Reconcile orderPos by mediaId (the queue index) and start a fresh listen.
    onTransition: (mediaId) => {
      const qi = Number(mediaId)
      const { order, queue, orderPos } = get()
      const pos = order.indexOf(qi)
      // pos === orderPos: the initial item or a repeat-one loop — onPlay already
      // handled it, so don't double-count the play / scrobble.
      if (pos < 0 || pos === orderPos) return
      const track = queue[qi]
      if (!track) return
      set({ orderPos: pos, currentTrack: track, isPlaying: true, loadError: null })
      if (!isEpisode(track)) {
        void window.aether.recordPlay(track.id)
        beginListen(track)
      }
      persistQueue()
    }
  })

  const persistQueue = (): void => {
    if (queueSaveTimer) window.clearTimeout(queueSaveTimer)
    queueSaveTimer = window.setTimeout(() => {
      const { queue, order, orderPos, shuffle, repeat } = get()
      void window.aether.saveQueueState({
        trackIds: queue.map((t) => t.id),
        order,
        orderPos,
        shuffle,
        repeat
      })
    }, 800)
  }

  // ---- native ExoPlayer queue mirroring (mobile only) ----
  // The native playlist mirrors the full play `order`, so currentMediaItemIndex
  // tracks orderPos and ExoPlayer auto-advances gapless in the background without
  // the (frozen) WebView. mediaId = the queue index (String(order[pos])), unique
  // per entry and stable until the next queue mutation re-pushes fresh ids.
  const buildEntries = (positions: number[]): QueueEntry[] => {
    const { queue, order } = get()
    const out: QueueEntry[] = []
    for (const pos of positions) {
      const qi = order[pos]
      const track = queue[qi]
      if (track) out.push({ track, mediaId: String(qi) })
    }
    return out
  }

  /** Push the entire order to the native queue and start at startPos. */
  const syncNativeQueue = (startPos: number): void => {
    if (!isMobile) return
    const { order, repeat } = get()
    playerEngine.setRepeatMode(repeat)
    playerEngine.setQueue(
      buildEntries(order.map((_, i) => i)),
      startPos
    )
  }

  /** Replace just the items after the current one (enqueue/playNext/reorder/shuffle). */
  const syncUpcoming = (): void => {
    if (!isMobile) return
    const { order, orderPos } = get()
    const positions: number[] = []
    for (let i = orderPos + 1; i < order.length; i++) positions.push(i)
    playerEngine.updateUpcoming(buildEntries(positions))
  }

  const startAt = (orderPos: number): void => {
    const { queue, order } = get()
    const track = queue[order[orderPos]]
    if (!track) return
    set({ orderPos, currentTrack: track, isPlaying: true })
    engineLoaded = true
    if (isMobile) {
      // Native ExoPlayer owns the queue and auto-advances in the background.
      syncNativeQueue(orderPos)
    } else {
      playerEngine.play(track)
    }
    persistQueue()
  }

  const persistVolume = (): void => {
    if (volumeSaveTimer) window.clearTimeout(volumeSaveTimer)
    volumeSaveTimer = window.setTimeout(() => {
      const { volume, muted } = get()
      void window.aether.setSettings({ volume, muted })
    }, 800)
  }

  return {
    queue: [],
    order: [],
    orderPos: -1,
    currentTrack: null,
    isPlaying: false,
    repeat: 'off',
    shuffle: false,
    volume: 0.8,
    muted: false,
    playbackRate: 1,
    sleepEndsAt: null,
    loadError: null,

    playTracks: (tracks, startIndex = 0) => {
      if (tracks.length === 0) return
      const order = get().shuffle
        ? shuffledOrder(tracks.length, startIndex)
        : tracks.map((_, i) => i)
      const orderPos = get().shuffle ? 0 : startIndex
      set({ queue: tracks, order })
      startAt(orderPos)
    },

    playQueueIndex: (queueIndex) => {
      const { order } = get()
      const pos = order.indexOf(queueIndex)
      if (pos >= 0) startAt(pos)
    },

    enqueue: (tracks) => {
      const { queue, order, currentTrack } = get()
      if (!currentTrack && queue.length === 0) {
        get().playTracks(tracks)
        return
      }
      const base = queue.length
      set({
        queue: [...queue, ...tracks],
        order: [...order, ...tracks.map((_, i) => base + i)]
      })
      persistQueue()
      syncUpcoming()
    },

    playNext: (tracks) => {
      const { queue, order, orderPos, currentTrack } = get()
      if (!currentTrack && queue.length === 0) {
        get().playTracks(tracks)
        return
      }
      const base = queue.length
      const newOrder = [...order]
      newOrder.splice(orderPos + 1, 0, ...tracks.map((_, i) => base + i))
      set({ queue: [...queue, ...tracks], order: newOrder })
      persistQueue()
      syncUpcoming()
    },

    removeFromQueue: (queueIndex) => {
      const { queue, order, orderPos } = get()
      const newQueue = queue.filter((_, i) => i !== queueIndex)
      const removedPos = order.indexOf(queueIndex)
      const newOrder = order
        .filter((i) => i !== queueIndex)
        .map((i) => (i > queueIndex ? i - 1 : i))
      const removedCurrent = removedPos === orderPos

      // Removed the last remaining track: stop playback and clear state, else
      // currentTrack stays stale and the engine keeps playing the removed audio.
      if (newOrder.length === 0) {
        finalizeListen()
        playerEngine.stop()
        engineLoaded = false
        set({ queue: newQueue, order: newOrder, orderPos: -1, currentTrack: null, isPlaying: false })
        persistQueue()
        return
      }

      let newPos = orderPos
      if (removedPos >= 0 && removedPos < orderPos) newPos--
      else if (removedPos === orderPos) newPos = Math.min(newPos, newOrder.length - 1)
      set({ queue: newQueue, order: newOrder, orderPos: newPos })
      persistQueue()
      // Removing the current track restarts at the new current on both platforms
      // (startAt updates currentTrack + the engine). Otherwise just refresh the
      // upcoming items (fresh mediaIds) — the current track keeps playing.
      if (removedCurrent) startAt(newPos)
      else syncUpcoming()
    },

    reorderQueue: (from, to) => {
      // from/to are positions in the *visible* (order) list
      const { order, orderPos } = get()
      const newOrder = [...order]
      const [moved] = newOrder.splice(from, 1)
      newOrder.splice(to, 0, moved)
      let newPos = orderPos
      if (from === orderPos) newPos = to
      else {
        if (from < orderPos) newPos--
        if (to <= newPos) newPos++
      }
      set({ order: newOrder, orderPos: newPos })
      persistQueue()
      syncUpcoming()
    },

    clearQueue: () => {
      finalizeListen()
      playerEngine.stop()
      engineLoaded = false
      set({
        queue: [],
        order: [],
        orderPos: -1,
        currentTrack: null,
        isPlaying: false
      })
      persistQueue()
    },

    togglePlay: () => {
      const { isPlaying, currentTrack, queue, orderPos } = get()
      if (!currentTrack) {
        if (queue.length > 0) startAt(0)
        return
      }
      if (isPlaying) {
        playerEngine.pause()
      } else if (!engineLoaded) {
        // restored queue: nothing loaded in the engine yet
        startAt(Math.max(0, orderPos))
      } else {
        playerEngine.resume()
      }
    },

    next: (manual = true) => {
      const { order, orderPos, repeat } = get()
      if (!manual && repeat === 'one') {
        // each completed repeat-one listen is independently scrobbleable
        finalizeListen()
        playerEngine.seek(0)
        playerEngine.resume()
        return
      }
      if (orderPos + 1 < order.length) {
        startAt(orderPos + 1)
      } else if (repeat === 'all' && order.length > 0) {
        startAt(0)
      } else {
        // end of queue: no upcoming onPlay will finalize this listen
        finalizeListen()
        set({ isPlaying: false })
      }
    },

    previous: () => {
      const { orderPos } = get()
      if (playerEngine.position() > 3 || orderPos <= 0) {
        playerEngine.seek(0)
      } else {
        startAt(orderPos - 1)
      }
    },

    seekBy: (delta) => {
      const pos = playerEngine.position()
      const dur = playerEngine.duration()
      playerEngine.seek(Math.max(0, Math.min(dur - 0.2, pos + delta)))
    },

    setVolume: (v) => {
      const vol = Math.max(0, Math.min(1, v))
      set({ volume: vol, muted: false })
      playerEngine.setVolume(vol, false)
      persistVolume()
    },

    adjustVolume: (delta) => {
      get().setVolume(get().volume + delta)
    },

    toggleMute: () => {
      const muted = !get().muted
      set({ muted })
      playerEngine.setVolume(get().volume, muted)
      persistVolume()
    },

    cycleRepeat: () => {
      const order: RepeatMode[] = ['off', 'all', 'one']
      const next = order[(order.indexOf(get().repeat) + 1) % order.length]
      set({ repeat: next })
      persistQueue()
      // Mobile: repeat is owned by the native ExoPlayer so it loops in the
      // background without the WebView.
      if (isMobile) playerEngine.setRepeatMode(next)
    },

    toggleShuffle: () => {
      const { shuffle, queue, order, orderPos } = get()
      const currentQueueIndex = order[orderPos] ?? 0
      if (!shuffle) {
        const newOrder = shuffledOrder(queue.length, currentQueueIndex)
        set({ shuffle: true, order: newOrder, orderPos: 0 })
      } else {
        set({
          shuffle: false,
          order: queue.map((_, i) => i),
          orderPos: currentQueueIndex
        })
      }
      persistQueue()
      // Current track stays at the new orderPos and keeps playing; just reshuffle
      // the upcoming items natively.
      syncUpcoming()
    },

    setPlaybackRate: (rate) => {
      const r = Math.max(0.5, Math.min(2, rate))
      set({ playbackRate: r })
      playerEngine.setRate(r)
    },

    setSleepTimer: (minutes) => {
      if (sleepTimer) {
        window.clearTimeout(sleepTimer)
        sleepTimer = null
      }
      if (minutes == null) {
        set({ sleepEndsAt: null })
        return
      }
      const fadeSec = 30
      const ms = Math.max(0, minutes * 60 - fadeSec) * 1000
      set({ sleepEndsAt: Date.now() + minutes * 60 * 1000 })
      sleepTimer = window.setTimeout(() => {
        playerEngine.fadeOutAndPause(fadeSec)
        set({ sleepEndsAt: null })
      }, ms)
    },

    peekNext: () => {
      const { queue, order, orderPos, repeat } = get()
      if (repeat === 'one') return get().currentTrack
      if (orderPos + 1 < order.length) return queue[order[orderPos + 1]] ?? null
      if (repeat === 'all' && order.length > 0) return queue[order[0]] ?? null
      return null
    },

    updateTrack: (track) => {
      const { queue, currentTrack } = get()
      set({
        queue: queue.map((t) => (t.id === track.id ? track : t)),
        currentTrack: currentTrack?.id === track.id ? track : currentTrack
      })
    },

    restoreQueue: (tracks, persisted) => {
      if (get().queue.length > 0) return // something already playing, don't clobber
      // keep only ids that still exist, remapping order indices accordingly
      const byId = new Map(tracks.map((t) => [t.id, t]))
      const queue: Track[] = []
      const indexMap = new Map<number, number>() // old index -> new index
      persisted.trackIds.forEach((id, oldIdx) => {
        const track = byId.get(id)
        if (track) {
          indexMap.set(oldIdx, queue.length)
          queue.push(track)
        }
      })
      if (queue.length === 0) return
      const order: number[] = []
      let orderPos = 0
      persisted.order.forEach((oldIdx, pos) => {
        const newIdx = indexMap.get(oldIdx)
        if (newIdx != null) {
          if (pos <= persisted.orderPos) orderPos = order.length
          order.push(newIdx)
        }
      })
      engineLoaded = false
      set({
        queue,
        order,
        orderPos: Math.min(orderPos, order.length - 1),
        currentTrack: queue[order[Math.min(orderPos, order.length - 1)]] ?? null,
        isPlaying: false,
        shuffle: persisted.shuffle,
        repeat: persisted.repeat
      })
    }
  }
})
