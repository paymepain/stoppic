// Все иконки интерфейса на сетке 24×24. Контурные рисуются обводкой currentColor,
// сплошные (solid) — заливкой currentColor; отдельные детали могут переопределять fill/stroke.

type IconDef = { body: string; solid?: boolean }

const ICONS = {
  // Инструменты оверлея
  select: { body: '<path d="M6 4v14.5l4-3.8 2.7 6.3 2.6-1.1-2.7-6.2H18z"/>' },
  arrow: { body: '<path d="M5 19l9.5-9.5"/><path d="M20 4v10L10 4z" fill="currentColor" stroke="none"/>' },
  line: { body: '<path d="M8 16L16 8"/><rect x="3" y="16" width="5" height="5" fill="currentColor" stroke="none"/><rect x="16" y="3" width="5" height="5" fill="currentColor" stroke="none"/>' },
  rect: { body: '<rect x="4" y="6" width="16" height="12"/>' },
  pen: { body: '<path d="M4 19c2.5-6 5.5-6.5 7-3.5s4.5 3 6-4"/><circle cx="18.5" cy="7.5" r="2.5" fill="currentColor" stroke="none"/>' },
  image: { body: '<rect x="4" y="5" width="16" height="14"/><path d="M4 16l5-5 4 4 2-2 5 5"/><circle cx="15.5" cy="9.5" r="1.8" fill="currentColor" stroke="none"/>' },
  marker: { body: '<g transform="translate(0 2) rotate(45 12 12)"><path d="M9 2h6v9H9z"/><path d="M9 12h6l-1.5 3v2l-3 1.5V15z" fill="currentColor" stroke="none"/></g><path d="M3 19.5h8v2H3z" fill="currentColor" stroke="none"/>' },
  text: { body: '<path d="M5 8V5h14v3M12 5v14M9 19h6"/>' },
  blur: { solid: true, body: '<path d="M8.2 3a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM12.9 3a0.9 0.9 0 1 0 -1.8 0a0.9 0.9 0 1 0 1.8 0zM17.2 3a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM3.7 7.5a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM8.7 7.5a1.2 1.2 0 1 0 -2.4 0a1.2 1.2 0 1 0 2.4 0zM13.55 7.5a1.55 1.55 0 1 0 -3.1 0a1.55 1.55 0 1 0 3.1 0zM17.7 7.5a1.2 1.2 0 1 0 -2.4 0a1.2 1.2 0 1 0 2.4 0zM21.7 7.5a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM3.9 12a0.9 0.9 0 1 0 -1.8 0a0.9 0.9 0 1 0 1.8 0zM9.05 12a1.55 1.55 0 1 0 -3.1 0a1.55 1.55 0 1 0 3.1 0zM14.1 12a2.1 2.1 0 1 0 -4.2 0a2.1 2.1 0 1 0 4.2 0zM18.05 12a1.55 1.55 0 1 0 -3.1 0a1.55 1.55 0 1 0 3.1 0zM21.9 12a0.9 0.9 0 1 0 -1.8 0a0.9 0.9 0 1 0 1.8 0zM3.7 16.5a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM8.7 16.5a1.2 1.2 0 1 0 -2.4 0a1.2 1.2 0 1 0 2.4 0zM13.55 16.5a1.55 1.55 0 1 0 -3.1 0a1.55 1.55 0 1 0 3.1 0zM17.7 16.5a1.2 1.2 0 1 0 -2.4 0a1.2 1.2 0 1 0 2.4 0zM21.7 16.5a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM8.2 21a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0zM12.9 21a0.9 0.9 0 1 0 -1.8 0a0.9 0.9 0 1 0 1.8 0zM17.2 21a0.7 0.7 0 1 0 -1.4 0a0.7 0.7 0 1 0 1.4 0z"/>' },
  number: { solid: true, body: '<path fill-rule="evenodd" d="M12 3a9 9 0 1 1 0 18a9 9 0 1 1 0-18zM10 7.5h3.6v9h-2.2v-6.8H10z"/>' },

  // Действия
  undo: { body: '<path d="M8 5L4 9l4 4"/><path d="M5 9h9a5 5 0 0 1 0 10h-4"/>' },
  redo: { body: '<path d="M16 5l4 4-4 4"/><path d="M19 9h-9a5 5 0 0 0 0 10h4"/>' },
  copy: { body: '<rect x="9" y="9" width="11" height="11"/><path d="M5 15V5h10"/>' },
  save: { body: '<path d="M12 4v10M7.5 10l4.5 4.5 4.5-4.5M5 19.5h14"/>' },
  pin: { body: '<path d="M12 14v6.5"/><path d="M8 3.5h8l-1 6.5 3 4H6l3-4z"/>' },
  close: { body: '<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>' },
  plus: { body: '<path d="M12 4v16M4 12h16"/>' },
  reset: { body: '<path d="M5.5 12a6.5 6.5 0 1 0 2-4.7"/><path d="M4 4v5h5z" fill="currentColor" stroke="none"/>' },

  // Запись
  mic: { body: '<rect x="9" y="3.5" width="6" height="11" rx="3"/><path d="M6 11a6 6 0 0 0 12 0M12 17.5v3"/>' },
  sys: { body: '<path d="M4 9h3.5L12 5v14l-4.5-4H4z" fill="currentColor" stroke="none"/><path d="M15.5 9.5a3.5 3.5 0 0 1 0 5M18 7a7 7 0 0 1 0 10"/>' },
  record: { solid: true, body: '<circle cx="12" cy="12" r="6.5"/>' },
  pause: { solid: true, body: '<path d="M6 5h4v14H6zM14 5h4v14h-4z"/>' },
  play: { solid: true, body: '<path d="M7 5l12 7-12 7z"/>' },
  stop: { solid: true, body: '<path d="M6 6h12v12H6z"/>' }
} satisfies Record<string, IconDef>

export type IconName = keyof typeof ICONS

/** Разметка <svg> для вставки через innerHTML. stroke — толщина обводки в единицах сетки 24. */
export function icon(name: IconName, size = 18, { stroke = 2, cls = '' } = {}): string {
  const def: IconDef = ICONS[name]
  const paint = def.solid
    ? 'fill="currentColor"'
    : `fill="none" stroke="currentColor" stroke-width="${stroke}" stroke-linecap="square" stroke-linejoin="miter"`
  const klass = cls ? ` class="${cls}"` : ''
  return `<svg${klass} viewBox="0 0 24 24" width="${size}" height="${size}" ${paint}>${def.body}</svg>`
}
