// Libellés lisibles des touches (noms internes du daemon → affichage).
const KEY_LABEL: Record<string, string> = {
  ControlLeft: 'Ctrl',
  ControlRight: 'Ctrl droit',
  ShiftLeft: 'Maj',
  ShiftRight: 'Maj droite',
  AltLeft: 'Alt',
  AltRight: 'AltGr',
  MetaLeft: 'Win',
  MetaRight: 'Win droit',
  Space: 'Espace',
  Enter: 'Entrée',
  Escape: 'Échap',
  ScrollLock: 'Verr. Défil',
  Insert: 'Inser',
  Pause: 'Pause',
  CapsLock: 'Verr. Maj',
  ContextMenu: 'Menu',
  Mouse4: 'Souris 4',
  Mouse5: 'Souris 5',
  MouseMiddle: 'Clic molette',
};

export const keyLabel = (k: string) => KEY_LABEL[k] ?? k.replace(/^Key/, '').replace(/^Digit/, '');
export const comboLabel = (keys: string[]) => keys.map(keyLabel).join(' + ');
