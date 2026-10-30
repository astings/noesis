// Fake Tauri backend for README screenshots. Injected before the Next.js UI loads, it answers
// every invoke() from invented demo data, so no real meeting ever reaches a picture.
// window.__demo.emit(event, payload) fires a Tauri event; window.__demo.unknown lists commands
// that fell through to the null default.
(() => {
  const OWNER = 'Alex';
  const projects = ['_inbox', 'acme', 'hiring', 'northwind'];

  // [seconds, speaker, text]
  const pricing = [
    [0, 'me', 'Thanks for making the time. I wanted to go through the pricing proposal before Friday.'],
    [6, 'them', 'Sure. We read it on Tuesday. The structure works for us, the per-seat number is the sticking point.'],
    [14, 'me', 'Understood. Is it the level or the way it scales with seats?'],
    [19, 'them', 'Mostly the scaling. Past two hundred seats we pay the same rate, and procurement will push back on that.'],
    [28, 'me', 'That is fair. We could add a tier at two hundred and fifty with a lower rate.'],
    [34, 'them', 'That would help. Could you also cap the annual increase? Last year we had eight percent.'],
    [41, 'me', 'I can propose a three percent cap for a two-year term.'],
    [46, 'them', 'Two years is fine. Our legal team will want the cap written into the order form, not the side letter.'],
    [54, 'me', 'Noted, order form it is. What about the onboarding fee?'],
    [59, 'them', 'We would like it waived, or credited against the first invoice.'],
    [65, 'me', 'Credited against the first invoice works on our side.'],
    [70, 'them', 'Great. Then the open item is the support tier. Do we need premium, or is standard enough?'],
    [78, 'me', 'For your volume, standard plus a named contact should be enough. I will put both options in the revision.'],
    [86, 'them', 'Good. Can we have the revised proposal by Thursday noon?'],
    [91, 'me', 'Yes. I will send it Thursday morning, with the new tier, the cap and the onboarding credit.'],
    [98, 'them', 'Perfect. I will book thirty minutes with procurement for Friday.'],
    [104, 'me', 'Thanks, Dana. Talk Friday.'],
  ];
  const standup = [
    [0, 'me', 'Quick one today. The import bug is fixed and deployed.'],
    [5, 'them', 'Nice. Any risk for the Monday release?'],
    [9, 'me', 'Only the migration. I will run it on staging tonight.'],
  ];
  const interview = [
    [0, 'me', 'Thanks for joining. Can you walk me through the last system you designed?'],
    [7, 'them', 'Sure. It was an event pipeline for invoices, about two million events a day.'],
    [15, 'me', 'What broke first when the volume grew?'],
    [19, 'them', 'The consumer lag. We moved from one partition per tenant to hashing by account.'],
  ];

  const at = (d) => new Date(d).toISOString();
  const meetings = [
    { id: 'meeting-demo-1', title: 'Pricing review with Acme', project: 'acme', started_at: at('2026-10-30T10:00:00-04:00'), duration_seconds: 2280, tags: ['pricing', 'procurement', 'renewal'], segs: pricing },
    { id: 'meeting-demo-2', title: 'Backend candidate, system design', project: 'hiring', started_at: at('2026-10-29T15:30:00-04:00'), duration_seconds: 3120, tags: ['interview', 'system-design'], segs: interview },
    { id: 'meeting-demo-3', title: 'Team standup', project: 'northwind', started_at: at('2026-10-29T09:15:00-04:00'), duration_seconds: 840, tags: ['standup', 'release'], segs: standup },
    { id: 'meeting-demo-4', title: 'Acme kickoff', project: 'acme', started_at: at('2026-10-27T14:00:00-04:00'), duration_seconds: 2700, tags: ['kickoff', 'scope'], segs: pricing.slice(0, 6) },
    { id: 'meeting-demo-5', title: 'Northwind roadmap sync', project: 'northwind', started_at: at('2026-10-24T11:00:00-04:00'), duration_seconds: 1980, tags: ['roadmap'], segs: standup },
    { id: 'meeting-demo-6', title: 'Call with Sam', project: '_inbox', started_at: at('2026-10-23T17:00:00-04:00'), duration_seconds: 600, tags: [], segs: standup.slice(0, 2) },
  ];
  const byId = (id) => meetings.find((m) => m.id === id);
  const folder = (m) => `/Users/demo/Movies/noesis-recordings/${m.project}/${m.started_at.slice(0, 10)}_${m.title.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`;
  const clock = (start, s) => new Date(new Date(start).getTime() + s * 1000).toTimeString().slice(0, 8);
  const transcripts = (m) =>
    m.segs.map(([s, speaker, text], i) => {
      const end = m.segs[i + 1] ? m.segs[i + 1][0] : s + 5;
      return { id: `${m.id}-t${i}`, text, timestamp: clock(m.started_at, s), sequence_id: i, audio_start_time: s, audio_end_time: end, duration: end - s, speaker, confidence: 0.95 };
    });

  const prefs = {
    save_folder: '/Users/demo/Movies/noesis-recordings', auto_save: true, file_format: 'mp4',
    preferred_mic_device: 'MacBook Pro Microphone', preferred_system_device: 'MacBook Pro Speakers',
    auto_enhance: true, enhance_provider: 'whisper', enhance_model: 'large-v3', enhance_language: null,
    auto_stop_on_call_end: true, auto_start_on_call: false, calendar_meeting_name: false, owner_name: OWNER,
    auto_title_tags: true, delete_audio_after_enhance: false, audio_retention_hours: 0,
    transcription_languages: ['en'], title_tags_prompt: '', ask_provider: '', ask_model: '',
  };

  const answers = {
    default: 'Dana agreed to the structure. Open points: a 250-seat tier with a lower rate, a 3% annual cap for a two-year term written into the order form, and the onboarding fee credited against the first invoice. You send the revised proposal Thursday morning; Dana books procurement for Friday.',
  };

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const handlers = {
    get_onboarding_status: () => ({ version: '1', completed: true, current_step: 0, model_status: { parakeet: 'downloaded', summary: 'downloaded' }, last_updated: at('2026-10-01') }),
    check_first_launch: () => false,
    get_recording_preferences: () => prefs,
    set_recording_preferences: () => null,
    list_projects_command: () => projects,
    get_project_settings: () => ({ language: null, enhance_provider: null, enhance_model: null }),
    api_get_meetings: () => meetings.map(({ segs, tags, ...m }) => ({ ...m, folder_path: folder(m) })),
    list_enhance_statuses: () => meetings.map((m) => ({ meeting_id: m.id, status: 'done' })),
    list_tagging_statuses: () => meetings.map((m) => ({ meeting_id: m.id, status: 'done' })),
    api_get_meeting_metadata: ({ meetingId }) => { const m = byId(meetingId); return { id: m.id, title: m.title, created_at: m.started_at, updated_at: m.started_at, folder_path: folder(m) }; },
    api_get_meeting_transcripts: ({ meetingId }) => { const t = transcripts(byId(meetingId)); return { transcripts: t, total_count: t.length, has_more: false }; },
    api_get_meeting: ({ meetingId }) => { const m = byId(meetingId); return { id: m.id, title: m.title, created_at: m.started_at, updated_at: m.started_at, transcripts: transcripts(m) }; },
    get_meeting_extras: ({ meetingId }) => ({
      tags: byId(meetingId).tags, audio_deleted_at: null, has_audio: true,
      transcription: { live: { provider: 'parakeet', model: 'parakeet-tdt-0.6b-v3-int8', language: 'en' }, enhanced: { provider: 'whisper', model: 'large-v3', status: 'done', language: 'en' } },
    }),
    api_search_transcripts: ({ query }) => {
      const q = String(query).toLowerCase();
      return meetings.flatMap((m) => {
        const hits = transcripts(m).filter((t) => t.text.toLowerCase().includes(q)).slice(0, 3)
          .map((t) => ({ start_seconds: t.audio_start_time, end_seconds: t.audio_end_time, snippet: t.text, text_match: true }));
        const title_match = m.title.toLowerCase().includes(q);
        if (!hits.length && !title_match) return [];
        return [{ id: m.id, title: m.title, matchContext: hits[0]?.snippet ?? '', timestamp: m.started_at, tags: m.tags, start_seconds: hits[0]?.start_seconds ?? 0, hits, title_match, matched_tags: [] }];
      });
    },
    ask_meeting: async () => { await sleep(1400); return { answer: answers.default, from_seconds: 0, to_seconds: 2280, segments: pricing.length }; },
    list_jobs: () => [],
    get_audio_devices: () => [
      { name: 'MacBook Pro Microphone', device_type: 'Input' },
      { name: 'MacBook Pro Speakers', device_type: 'Output' },
    ],
    is_recording: () => false,
    get_recording_state: () => ({ is_recording: false, is_paused: false, is_active: false, recording_duration: null, active_duration: null }),
    get_transcription_status: () => ({ chunks_in_queue: 0, is_processing: false, last_activity_ms: 0 }),
    get_recording_meeting_name: () => null,
    get_transcript_history: () => [],
    parakeet_has_available_models: () => true,
    whisper_has_available_models: () => true,
    parakeet_is_model_loaded: () => true,
    whisper_is_model_loaded: () => true,
    parakeet_get_available_models: () => [],
    whisper_get_available_models: () => [],
    api_get_transcript_config: () => ({ provider: 'parakeet', model: 'parakeet-tdt-0.6b-v3-int8', apiKey: null }),
    api_get_model_config: () => ({ provider: 'builtin-ai', model: 'qwen3.5:4b', whisperModel: 'large-v3', apiKey: null, ollamaEndpoint: null }),
    get_default_recordings_folder_path: () => prefs.save_folder,
    get_meeting_folder_path: () => null,
    has_audio_checkpoints: () => false,
    get_notification_settings: () => null,
  };

  // Event plugin: listen registers a callback id; emit runs it with {event, id, payload}.
  const callbacks = new Map();
  const listeners = new Map();
  let nextId = 1;
  const unknown = new Set();

  async function invoke(cmd, args = {}) {
    if (cmd === 'plugin:event|listen') {
      (listeners.get(args.event) ?? listeners.set(args.event, []).get(args.event)).push(args.handler);
      return args.handler;
    }
    if (cmd === 'plugin:event|unlisten') {
      const l = listeners.get(args.event) ?? [];
      const i = l.indexOf(args.eventId ?? args.id);
      if (i >= 0) l.splice(i, 1);
      return null;
    }
    if (cmd.startsWith('plugin:')) return null;
    const h = handlers[cmd];
    if (!h) { unknown.add(cmd); return null; }
    return structuredClone(await h(args));
  }

  window.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(cb, once) {
      const id = nextId++;
      callbacks.set(id, (data) => { if (once) callbacks.delete(id); return cb && cb(data); });
      return id;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    runCallback: (id, data) => callbacks.get(id)?.(data),
    callbacks,
    convertFileSrc: (p) => p,
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_e, id) => callbacks.delete(id) };
  window.__demo = {
    unknown,
    emit(event, payload) {
      for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
    },
  };
})();
