(() => {
  'use strict';

  const dictionaries = window.siteI18n;
  const languages = ['ru', 'en', 'uk', 'pl'];
  const themes = ['dark', 'light', 'cb-dark', 'cb-light', 'mc'];
  const latestRelease = 'https://github.com/loxord241/Cobble-Launcher/releases/latest';
  const releaseApi = 'https://api.github.com/repos/loxord241/Cobble-Launcher/releases/latest';
  const cacheKey = 'siteRelease';
  const cacheLifetime = 60 * 60 * 1000;
  const storage = {
    get(key) { try { return localStorage.getItem(key); } catch { return null; } },
    set(key, value) { try { localStorage.setItem(key, value); } catch { /* Private browsing: keep the current session usable. */ } }
  };
  const savedLanguage = storage.get('siteLang');
  // Дефолт сайта — английский: без navigator-угадайки, сохранённый выбор главнее.
  let language = languages.includes(savedLanguage) ? savedLanguage : 'en';
  let releaseVersion = null;
  let releaseState = 'loading';
  let activeScreenshot = null;
  let galleryOpener = null;

  const languageSelect = document.querySelector('#language');
  const video = document.querySelector('#demo');
  const videoButton = document.querySelector('#video-toggle');
  const dialog = document.querySelector('#lightbox');
  const dialogImage = document.querySelector('#lightbox-image');
  const dialogCaption = document.querySelector('#lightbox-caption');
  const motionPreference = window.matchMedia('(prefers-reduced-motion: reduce)');

  function t(key) {
    return dictionaries[language]?.[key] ?? dictionaries.en[key] ?? '';
  }

  function updateReleaseView() {
    const label = document.querySelector('#release-version');
    label.textContent = releaseVersion ? t('release.version').replace('{version}', releaseVersion) : t(`release.${releaseState}`);
    document.querySelector('[data-download]').href = releaseVersion
      ? `${latestRelease}/download/Cobble.Launcher_${encodeURIComponent(releaseVersion)}_x64-setup.exe`
      : latestRelease;
  }

  function updateVideoButton() {
    const key = video.paused ? 'video.play' : 'video.pause';
    videoButton.dataset.i18nAria = key;
    videoButton.setAttribute('aria-label', t(key));
    videoButton.setAttribute('aria-pressed', String(video.paused));
  }

  function updateLightbox() {
    if (!activeScreenshot) return;
    dialogImage.alt = t(`gallery.${activeScreenshot}.alt`);
    dialogCaption.textContent = t(`gallery.${activeScreenshot}`);
  }

  // Медиа (видео hero и скриншоты галереи) переключаются вместе с языком.
  // ru-версии есть только у ru; uk/pl показывают en-варианты.
  const mediaLanguage = () => (language === 'ru' ? 'ru' : 'en');
  const capitalize = (value) => value.charAt(0).toUpperCase() + value.slice(1);

  function applyMedia() {
    const key = capitalize(mediaLanguage());
    const nextSrc = video.dataset[`video${key}`];
    const nextPoster = video.dataset[`poster${key}`];
    if (!nextSrc || video.dataset.mediaLang === mediaLanguage()) return;
    const time = video.currentTime;
    const wasPlaying = !video.paused && !video.ended;
    video.dataset.mediaLang = mediaLanguage();
    video.src = nextSrc;
    if (nextPoster) video.setAttribute('poster', nextPoster);
    video.addEventListener('loadedmetadata', () => {
      if (time > 0 && Number.isFinite(time)) video.currentTime = time;
      if (wasPlaying && !motionPreference.matches) video.play().catch(() => {});
    }, { once: true });
    video.load();
  }

  function applyGalleryMedia() {
    const key = capitalize(mediaLanguage());
    document.querySelectorAll('img[data-img-en]').forEach(image => {
      const next = image.dataset[`img${key}`];
      if (next) image.src = next;
    });
  }

  function applyLanguage(nextLanguage) {
    language = languages.includes(nextLanguage) ? nextLanguage : 'en';
    document.documentElement.lang = language;
    languageSelect.value = language;
    document.querySelectorAll('[data-i18n]').forEach(element => {
      element.textContent = t(element.dataset.i18n);
    });
    for (const [attribute, dataName] of [['aria-label', 'i18nAria'], ['alt', 'i18nAlt'], ['title', 'i18nTitle']]) {
      document.querySelectorAll(`[data-${dataName.replace(/[A-Z]/g, letter => '-' + letter.toLowerCase())}]`).forEach(element => {
        element.setAttribute(attribute, t(element.dataset[dataName]));
      });
    }
    document.title = `${t('brand')} — ${t('seo.title')}`;
    updateReleaseView();
    updateVideoButton();
    updateLightbox();
    if (activeScreenshot && galleryOpener) {
      const image = galleryOpener.querySelector('img');
      const next = image?.dataset[`img${capitalize(mediaLanguage())}`];
      if (next) dialogImage.src = next;
    }
    applyMedia();
    applyGalleryMedia();
  }

  function applyTheme(theme) {
    const choice = themes.includes(theme) ? theme : 'dark';
    if (choice === 'dark') document.documentElement.removeAttribute('data-theme');
    else document.documentElement.dataset.theme = choice;
    document.querySelectorAll('[data-theme-choice]').forEach(button => {
      button.setAttribute('aria-pressed', String(button.dataset.themeChoice === choice));
    });
  }

  function validVersion(version) {
    return typeof version === 'string' && version.length <= 80 && /^\d+\.\d+\.\d+(?:[-+][\da-z.-]+)?$/i.test(version);
  }

  async function loadRelease() {
    try {
      const cached = JSON.parse(storage.get(cacheKey));
      const age = Date.now() - cached?.fetchedAt;
      if (validVersion(cached?.version) && Number.isFinite(cached.fetchedAt) && age >= 0 && age < cacheLifetime) {
        releaseVersion = cached.version;
        updateReleaseView();
        return;
      }
    } catch { /* Ignore malformed or unavailable browser storage. */ }

    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 8000);
    try {
      const response = await fetch(releaseApi, {
        headers: { Accept: 'application/vnd.github+json' },
        credentials: 'omit', referrerPolicy: 'no-referrer', signal: controller.signal
      });
      if (!response.ok) throw new Error('Release request failed');
      const release = await response.json();
      const version = typeof release.tag_name === 'string' ? release.tag_name.replace(/^v/, '') : '';
      if (!validVersion(version)) throw new Error('Release tag is invalid');
      releaseVersion = version;
      storage.set(cacheKey, JSON.stringify({ version, fetchedAt: Date.now() }));
    } catch {
      releaseState = 'unavailable';
    } finally {
      window.clearTimeout(timeout);
      updateReleaseView();
    }
  }

  languageSelect.addEventListener('change', () => {
    applyLanguage(languageSelect.value);
    storage.set('siteLang', language);
  });
  document.querySelectorAll('[data-theme-choice]').forEach(button => {
    button.addEventListener('click', () => {
      applyTheme(button.dataset.themeChoice);
      storage.set('siteTheme', button.dataset.themeChoice);
    });
  });
  document.querySelectorAll('[data-gallery]').forEach(button => {
    button.addEventListener('click', () => {
      activeScreenshot = button.dataset.gallery;
      galleryOpener = button;
      const image = button.querySelector('img');
      dialogImage.src = image?.dataset[`img${capitalize(mediaLanguage())}`] ?? `./assets/img/${activeScreenshot}.png`;
      updateLightbox();
      if (!dialog.open) dialog.showModal();
      document.body.classList.add('has-dialog');
      dialog.querySelector('button').focus();
    });
  });
  dialog.querySelector('.close-button').addEventListener('click', () => dialog.close());
  dialog.addEventListener('keydown', event => {
    // The screenshot dialog has one interactive control. Keep Tab in the dialog.
    if (event.key === 'Tab') {
      event.preventDefault();
      dialog.querySelector('.close-button').focus();
    }
  });
  dialog.addEventListener('click', event => {
    const bounds = dialog.getBoundingClientRect();
    if (event.target === dialog && (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom)) dialog.close();
  });
  dialog.addEventListener('close', () => {
    document.body.classList.remove('has-dialog');
    galleryOpener?.focus({ preventScroll: true });
  });

  videoButton.addEventListener('click', () => {
    if (video.paused) video.play().catch(updateVideoButton);
    else video.pause();
  });
  video.addEventListener('play', updateVideoButton);
  video.addEventListener('pause', updateVideoButton);
  function respectMotionPreference() {
    if (motionPreference.matches) {
      video.autoplay = false;
      video.pause();
    }
    updateVideoButton();
  }
  motionPreference.addEventListener('change', respectMotionPreference);

  applyTheme(storage.get('siteTheme'));
  applyLanguage(language);
  respectMotionPreference();
  loadRelease();

  if ('IntersectionObserver' in window && !motionPreference.matches) {
    const observer = new IntersectionObserver(entries => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        entry.target.classList.add('arriving');
        observer.unobserve(entry.target);
      }
    }, { threshold: 0.08 });
    document.querySelectorAll('[data-reveal]').forEach(section => observer.observe(section));
  }
})();
