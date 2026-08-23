// Copyright 2026 VKrishna04
// SPDX-License-Identifier: Apache-2.0

import { useEffect, useState } from "react";
import { LazyMotion, domAnimation, m, useReducedMotion } from "framer-motion";

// Hand-written, not machine-translated, and deliberately only the pitch: the CLI
// itself speaks English, and claiming otherwise on the marketing page would be a
// promise the binary does not keep.
const LANGS = [
  {
    code: "en",
    label: "English",
    pitch:
      "Your disk is full of dependencies you can rebuild. dev-prune deletes them — only after the package manager proves a lockfile can put them back.",
    note: "Terminal output is English.",
  },
  {
    code: "zh-Hans",
    label: "中文",
    pitch:
      "你的硬盘里塞满了可以重新构建的依赖。dev-prune 会删除它们——但只在包管理器证明锁文件能够将其还原之后。",
    note: "命令行输出为英文。",
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
    note: "टर्मिनल आउटपुट अंग्रेज़ी में है।",
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
