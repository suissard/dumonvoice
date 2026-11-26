<script lang="ts">
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  const ios = /iPhone|iPad|iPod/.test(ua);
  const android = /Android/.test(ua);
</script>

<div class="mx-auto max-w-xl space-y-4 px-4 py-8">
  <h1 class="text-xl font-bold">Certificat local & installation</h1>
  <section class="card space-y-2 text-sm">
    <h2 class="font-semibold">Mode rapide (aucune installation)</h2>
    <p>
      Le PC utilise un certificat HTTPS généré localement. Au premier scan du QR code, votre navigateur affiche un avertissement :
      choisissez <em>Paramètres avancés → Continuer vers le site</em> (Chrome) ou <em>Afficher les détails → visiter ce site</em> (Safari).
      Le micro, le presse-papiers et la connexion sécurisée fonctionnent ensuite dans l’onglet.
    </p>
  </section>
  <section class="card space-y-3 text-sm">
    <h2 class="font-semibold">Mode application (optionnel)</h2>
    <p>Installez l’autorité locale pour supprimer l’avertissement et épingler DumonVoice sur l’écran d’accueil.</p>
    <a class="btn-primary" href="/ca.crt" download="DumonVoice-CA.crt">Télécharger le certificat (ca.crt)</a>
    {#if ios || !android}
      <div>
        <p class="font-semibold">iPhone / iPad</p>
        <ol class="ml-5 list-decimal space-y-1">
          <li>Ouvrez ce lien dans Safari et autorisez le téléchargement du profil.</li>
          <li>Réglages → Général → VPN et gestion de l’appareil → installez « DumonVoice Local CA ».</li>
          <li>Réglages → Général → Informations → Réglages des certificats → activez la confiance totale.</li>
          <li>Rouvrez le compagnon, puis Partager → « Sur l’écran d’accueil ».</li>
        </ol>
      </div>
    {/if}
    {#if android || !ios}
      <div>
        <p class="font-semibold">Android</p>
        <ol class="ml-5 list-decimal space-y-1">
          <li>Téléchargez le certificat.</li>
          <li>Paramètres → Sécurité → Chiffrement et identifiants → Installer un certificat → Certificat CA.</li>
          <li>Rouvrez le compagnon dans Chrome, puis menu ⋮ → « Installer l’application ».</li>
        </ol>
      </div>
    {/if}
    <p class="muted text-xs">
      Ce certificat n’est valable que pour votre PC ; la clé privée ne quitte jamais votre ordinateur.
    </p>
  </section>
  <a class="text-sm text-brand-600 underline" href="/remote">← Retour au compagnon</a>
</div>
