// Emoji for the chat: :shortcodes: (typed ones become the emoji when sent, as in Discord), from
// every emoji's own names (emoji-data.js) and Discord's for the common ones; ecdh.chat's own set
// stays for secret chats' reactions.

import { EMOJI } from './emoji-data.js';

// The reactions ecdh.chat's protocol carries (secure/js/messages.js REACTIONS); nothing else can
// be sent as one.
export { REACTIONS } from '../secure/js/messages.js';

// Discord's names for the common ones, over every emoji's own shortcodes (emoji-data.js).
const NAMED = {
  smile: '😄', smiley: '😃', grinning: '😀', grin: '😁', laughing: '😆', sweat_smile: '😅', joy: '😂',
  rofl: '🤣', slight_smile: '🙂', upside_down: '🙃', wink: '😉', blush: '😊', innocent: '😇',
  heart_eyes: '😍', star_struck: '🤩', kissing_heart: '😘', yum: '😋', stuck_out_tongue: '😛',
  zany_face: '🤪', hugging: '🤗', thinking: '🤔', neutral_face: '😐', expressionless: '😑',
  smirk: '😏', unamused: '😒', rolling_eyes: '🙄', grimacing: '😬', relieved: '😌', pensive: '😔',
  sleeping: '😴', mask: '😷', nauseated_face: '🤢', hot_face: '🥵', cold_face: '🥶', exploding_head: '🤯',
  partying_face: '🥳', sunglasses: '😎', nerd: '🤓', confused: '😕', worried: '😟', open_mouth: '😮',
  flushed: '😳', pleading_face: '🥺', cry: '😢', sob: '😭', scream: '😱', triumph: '😤', rage: '😡',
  skull: '💀', thumbsup: '👍', '+1': '👍', thumbsdown: '👎', '-1': '👎', ok_hand: '👌', v: '✌️',
  fingers_crossed: '🤞', metal: '🤘', call_me: '🤙', point_left: '👈', point_right: '👉', wave: '👋',
  raised_hand: '✋', clap: '👏', raised_hands: '🙌', pray: '🙏', muscle: '💪', eyes: '👀',
  heart: '❤️', orange_heart: '🧡', yellow_heart: '💛', green_heart: '💚', blue_heart: '💙',
  purple_heart: '💜', black_heart: '🖤', white_heart: '🤍', broken_heart: '💔', sparkling_heart: '💖',
  fire: '🔥', sparkles: '✨', star: '⭐', tada: '🎉', '100': '💯', lock: '🔒', key: '🔑', check: '✔️',
  white_check_mark: '✅', x: '❌', warning: '⚠️', question: '❓', exclamation: '❗', zzz: '💤',
  poop: '💩', ghost: '👻', robot: '🤖', clown: '🤡', cat: '🐱', dog: '🐶', pizza: '🍕', coffee: '☕',
  beer: '🍺', cake: '🍰', gift: '🎁', rocket: '🚀', moneybag: '💰', bulb: '💡', shrug: '🤷',
  facepalm: '🤦', salute: '🫡', melting_face: '🫠', saluting_face: '🫡', handshake: '🤝',
};

export const SHORTCODES = (() => {
  const out = {};
  for (const [char, , codes] of EMOJI) for (const name of codes.split(' ')) if (name && !(name in out)) out[name] = char;
  return Object.assign(out, NAMED);
})();

// :shortcode: in typed text becomes the emoji, the way Discord sends it.
export function replaceShortcodes(text) {
  return String(text).replace(/(^|[^\\\w]):([a-z0-9_+-]{1,32}):/g, (all, before, name) => (SHORTCODES[name] ? before + SHORTCODES[name] : all));
}
