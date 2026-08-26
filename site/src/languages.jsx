// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import { useEffect, useState } from "react";
import { LazyMotion, domAnimation, m, useReducedMotion } from "framer-motion";

// The pitch, in the languages dev-prune is read in. Twelve of these — English,
// Chinese and the Indic block — are also languages the binary prints its own headings in,
// and their note says so; the rest are the pitch only, because there is no catalogue
// for them yet. Nothing here has been proofread by a native speaker, which is the
// same thing `devp config set language` says out loud: docs/TRANSLATIONS.md is where
// a correction goes.
const LANGS = [
  {
    code: "en",
    label: "English",
    pitch:
      "Your disk is full of dependencies you can rebuild. dev-prune deletes them — only after the package manager proves a lockfile can put them back.",
    note: "The CLI speaks twelve languages: devp config set language en",
  },
  {
    code: "zh-Hans",
    label: "中文",
    pitch:
      "你的硬盘里塞满了可以重新构建的依赖。dev-prune 会删除它们——但只在包管理器证明锁文件能够将其还原之后。",
    note: "dev-prune 用简体中文打印自己的标题：devp config set language zh",
  },
  {
    code: "ja",
    label: "日本語",
    pitch:
      "ディスクを占めているのは、作り直せる依存関係です。dev-prune はそれらを削除します。ただし、ロックファイルから復元できるとパッケージマネージャーが証明した後だけです。",
    note: "コマンドラインの表示は英語です。",
  },
  {
    code: "ko",
    label: "한국어",
    pitch:
      "디스크를 채우고 있는 것은 다시 설치할 수 있는 의존성입니다. dev-prune은 그것들을 삭제합니다 — 잠금 파일로 복원할 수 있음을 패키지 매니저가 증명한 뒤에만.",
    note: "명령줄 출력은 영어입니다.",
  },
  {
    code: "es",
    label: "Español",
    pitch:
      "Tu disco está lleno de dependencias que puedes reconstruir. dev-prune las borra, pero solo después de que el gestor de paquetes demuestre que un lockfile puede devolverlas.",
    note: "La salida en la terminal está en inglés.",
  },
  {
    code: "pt-BR",
    label: "Português",
    pitch:
      "Seu disco está cheio de dependências que podem ser reconstruídas. O dev-prune as apaga — só depois que o gerenciador de pacotes provar que um lockfile consegue trazê-las de volta.",
    note: "A saída no terminal é em inglês.",
  },
  {
    code: "de",
    label: "Deutsch",
    pitch:
      "Deine Festplatte ist voll mit Abhängigkeiten, die sich neu bauen lassen. dev-prune löscht sie – aber erst, wenn der Paketmanager bewiesen hat, dass eine Lockfile sie zurückbringt.",
    note: "Die Ausgabe im Terminal ist auf Englisch.",
  },
  {
    code: "fr",
    label: "Français",
    pitch:
      "Votre disque est rempli de dépendances que vous pouvez reconstruire. dev-prune les supprime, mais seulement après que le gestionnaire de paquets a prouvé qu’un lockfile peut les restaurer.",
    note: "La sortie du terminal est en anglais.",
  },
  {
    code: "ru",
    label: "Русский",
    pitch:
      "Ваш диск забит зависимостями, которые можно собрать заново. dev-prune удаляет их — но только после того, как пакетный менеджер докажет, что lock-файл вернёт их на место.",
    note: "Вывод в терминале — на английском.",
  },
  {
    code: "hi",
    label: "हिन्दी",
    pitch:
      "आपकी डिस्क उन डिपेंडेंसी से भरी है जिन्हें दोबारा बनाया जा सकता है। dev-prune उन्हें हटाता है — लेकिन तभी, जब पैकेज मैनेजर यह साबित कर दे कि लॉकफ़ाइल उन्हें वापस ला सकती है।",
    note: "dev-prune अपने शीर्षक हिन्दी में छापता है: devp config set language hi",
  },
  {
    code: "te",
    label: "తెలుగు",
    pitch:
      "మీ డిస్క్ మళ్ళీ నిర్మించగలిగే డిపెండెన్సీలతో నిండిపోయింది. dev-prune వాటిని తొలగిస్తుంది — లాక్‌ఫైల్ వాటిని తిరిగి తీసుకురాగలదని ప్యాకేజ్ మేనేజర్ నిరూపించిన తర్వాతే.",
    note: "dev-prune తన శీర్షికలను తెలుగులో చూపుతుంది: devp config set language te",
  },
  {
    code: "ta",
    label: "தமிழ்",
    pitch:
      "உங்கள் வட்டு மீண்டும் உருவாக்கக்கூடிய சார்புகளால் நிரம்பியுள்ளது. dev-prune அவற்றை நீக்குகிறது — லாக்ஃபைல் அவற்றை மீட்டெடுக்க முடியும் என்பதைப் பொதி மேலாளர் நிரூபித்த பிறகுதான்.",
    note: "dev-prune தன் தலைப்புகளைத் தமிழில் அச்சிடுகிறது: devp config set language ta",
  },
  {
    code: "kn",
    label: "ಕನ್ನಡ",
    pitch:
      "ನಿಮ್ಮ ಡಿಸ್ಕ್ ಮತ್ತೆ ನಿರ್ಮಿಸಬಹುದಾದ ಅವಲಂಬನೆಗಳಿಂದ ತುಂಬಿದೆ. dev-prune ಅವುಗಳನ್ನು ಅಳಿಸುತ್ತದೆ — ಲಾಕ್‌ಫೈಲ್ ಅವುಗಳನ್ನು ಮರಳಿ ತರಬಲ್ಲದು ಎಂದು ಪ್ಯಾಕೇజ್ ಮ್ಯಾನೇజರ್ ಸಾಬೀತುಪಡಿಸಿದ ನಂತರವೇ.",
    note: "dev-prune ತನ್ನ ಶೀರ್ಷಿಕೆಗಳನ್ನು ಕನ್ನಡದಲ್ಲಿ ಮುದ್ರಿಸುತ್ತದೆ: devp config set language kn",
  },
  {
    code: "ml",
    label: "മലയാളം",
    pitch:
      "നിങ്ങളുടെ ഡിസ്ക് വീണ്ടും നിർമ്മിക്കാവുന്ന ഡിപൻഡൻസികളാൽ നിറഞ്ഞിരിക്കുന്നു. dev-prune അവ ഇല്ലാതാക്കുന്നു — ലോക്ക്ഫയലിന് അവ തിരികെ കൊണ്ടുവരാനാകുമെന്ന് പാക്കേജ് മാനേജർ തെളിയിച്ചതിനു ശേഷം മാത്രം.",
    note: "dev-prune തന്റെ തലക്കെട്ടുകൾ മലയാളത്തിൽ അച്ചടിക്കുന്നു: devp config set language ml",
  },
  {
    code: "bn",
    label: "বাংলা",
    pitch:
      "আপনার ডিস্ক এমন ডিপেন্ডেন্সিতে ভরা, যেগুলো আবার তৈরি করা যায়। dev-prune সেগুলো মুছে ফেলে — তবে কেবল প্যাকেজ ম্যানেজার প্রমাণ করার পরেই যে লকফাইল সেগুলো ফিরিয়ে আনতে পারে।",
    note: "dev-prune তার নিজস্ব শিরোনাম বাংলায় ছাপে: devp config set language bn",
  },
  {
    code: "mr",
    label: "मराठी",
    pitch:
      "तुमची डिस्क पुन्हा तयार करता येणाऱ्या डिपेंडन्सींनी भरलेली आहे. dev-prune त्या हटवते — पण फक्त पॅकेज मॅनेजरने हे सिद्ध केल्यावरच की लॉकफाईल त्या परत आणू शकते.",
    note: "dev-prune स्वतःची शीर्षके मराठीत छापते: devp config set language mr",
  },
  {
    code: "gu",
    label: "ગુજરાતી",
    pitch:
      "તમારી ડિસ્ક એવી ડિપેન્ડન્સીથી ભરેલી છે જે ફરીથી બનાવી શકાય છે. dev-prune તેમને કાઢી નાખે છે — પણ ત્યારે જ, જ્યારે પેકેજ મેનેજર સાબિત કરે કે લ્ટ ફાઇલ તેમને પાછી લાવી શકે છે.",
    note: "dev-prune પોતાનાં મથાળાં ગુજરાતીમાં છાપે છે: devp config set language gu",
  },
  {
    code: "pa",
    label: "ਪੰਜਾਬੀ",
    pitch:
      "ਤੁਹਾਡੀ ਡਿਸਕ ਉਹਨਾਂ ਡਿਪੈਂਡੈਂਸੀਆਂ ਨਾਲ ਭਰੀ ਹੋਈ ਹੈ ਜੋ ਦੁਬਾਰਾ ਬਣਾਈਆਂ ਜਾ ਸਕਦੀਆਂ ਹਨ। dev-prune ਉਹਨਾਂ ਨੂੰ ਮਿਟਾ ਦਿੰਦਾ ਹੈ — ਪਰ ਸਿਰਫ਼ ਉਦੋਂ, ਜਦੋਂ ਪੈਕੇਜ ਮੈਨੇਜਰ ਸਾਬਤ ਕਰ ਦੇਵੇ ਕਿ ਲਾਕਫ਼ਾਈਲ ਉਹਨਾਂ ਨੂੰ ਵਾਪਸ ਲਿਆ ਸਕਦੀ ਹੈ।",
    note: "dev-prune ਆਪਣੇ ਸਿਰਲੇਖ ਪੰਜਾਬੀ ਵਿੱਚ ਛਾਪਦਾ ਹੈ: devp config set language pa",
  },
  {
    code: "sa",
    label: "संस्कृतम्",
    pitch:
      "तव सङ्गणकस्य कोशः पुनर्निर्मातुं शक्यैः परावलम्बनैः पूर्णः अस्ति। dev-prune तानि अपनयति — किन्तु तदैव, यदा पेटिकाप्रबन्धकः प्रमाणयति यत् तालसञ्चिका तानि पुनः आनेतुं शक्नोति।",
    note: "dev-prune स्वकीयानि शीर्षकानि संस्कृते मुद्रयति: devp config set language sa",
  },
];

export default function Languages() {
  const [active, setActive] = useState("en");
  // The page is prerendered to static HTML, so a motion `initial` would bake
  // opacity:0 into the file a crawler reads. Nothing animates until mount.
  const [armed, setArmed] = useState(false);
  const reduce = useReducedMotion();
  useEffect(() => setArmed(true), []);
  const lang = LANGS.find((l) => l.code === active) ?? LANGS[0];

  return (
    <LazyMotion features={domAnimation} strict>
      <div className="langs">
        <div className="langs-tabs" role="tablist" aria-label="Pitch language">
          {LANGS.map((l) => (
            <button
              key={l.code}
              type="button"
              role="tab"
              lang={l.code}
              aria-selected={l.code === active}
              className={`langs-tab ${l.code === active ? "is-on" : ""}`}
              onClick={() => setActive(l.code)}
            >
              {l.label}
            </button>
          ))}
        </div>
        <m.blockquote
          key={lang.code}
          lang={lang.code}
          className="langs-pitch"
          initial={armed && !reduce ? { opacity: 0, y: 6 } : false}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
        >
          <p>{lang.pitch}</p>
          <footer lang={lang.code}>{lang.note}</footer>
        </m.blockquote>
      </div>
    </LazyMotion>
  );
}
