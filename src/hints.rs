//! The keymap as data: which-key lists, the `?` help and the bottom hint line
//! are all derived from [`BINDINGS`]. A feature that adds a key adds a row here
//! and it shows up everywhere.

use std::time::Duration;

/// How long a pending prefix waits before the which-key window opens.
pub const WHICH_KEY_DELAY: Duration = Duration::from_millis(300);

/// The internal name of the `Ctrl-w` prefix.
pub const CTRL_W: char = '\u{17}';

/// The internal name of the `Space p` prefix (Prompt keys).
pub const SPACE_P: char = '\u{10}';

/// The internal name of `z` in the file panel and the file picker: the same
/// key as the `z` of the commit list, with keys of its own.
pub const Z_TREE: char = '\u{1a}';

/// The `Space` keys that work while the file picker is open.
pub const PICKER_SPACE: &[&str] = &["g", "i"];

/// One key (or the last key of a chain).
pub struct Binding {
    /// The prefix key that must come first, if any.
    pub prefix: Option<char>,
    /// The key as shown, without the prefix.
    pub key: &'static str,
    pub label: &'static str,
}

const fn b(prefix: Option<char>, key: &'static str, label: &'static str) -> Binding {
    Binding { prefix, key, label }
}

pub const BINDINGS: &[Binding] = &[
    b(None, "R", "применить новые коммиты ветки (как :reload)"),
    b(Some(' '), "c", "выбор коммита (срез)"),
    b(Some(' '), "e", "показать/скрыть сайдбар"),
    b(Some(' '), "f", "выбор файла"),
    b(Some(' '), "g", "панель файлов: группы / дерево всех файлов"),
    b(
        Some(' '),
        "i",
        "описание группы под курсором (ещё раз или Esc: скрыть)",
    ),
    b(Some(' '), "p", "промпт: копировать / показать (подменю)"),
    b(Some(' '), "s", "саммари аналитика"),
    b(Some(' '), "u", "фильтр: только непросмотренные файлы"),
    b(Some(' '), "x", "аналитик: объяснить хунк под курсором"),
    b(
        Some(' '),
        "y",
        "скопировать выделение вместе с комментариями (VISUAL или мышь)",
    ),
    b(Some(' '), "X", "аналитик: объяснить файл под курсором"),
    b(
        Some(SPACE_P),
        "p",
        "скопировать промпт из всех открытых комментариев",
    ),
    b(
        Some(SPACE_P),
        "f",
        "скопировать промпт из открытых комментариев файла",
    ),
    b(
        Some(SPACE_P),
        "c",
        "скопировать промпт из комментария под курсором",
    ),
    b(
        Some(SPACE_P),
        "s",
        "показать промпт в оверлее (ничего не отправляется)",
    ),
    b(Some('g'), "g", "в начало файла (Ngg: к строке N)"),
    b(Some(']'), "c", "следующий коммит"),
    b(Some(']'), "f", "следующий файл"),
    b(Some(']'), "g", "следующая группа файлов"),
    b(Some(']'), "h", "следующий хунк"),
    b(Some(']'), "u", "следующий неотревьюенный файл"),
    b(Some('['), "c", "предыдущий коммит"),
    b(Some('['), "f", "предыдущий файл"),
    b(Some('['), "g", "предыдущая группа файлов"),
    b(Some('['), "h", "предыдущий хунк"),
    b(Some('['), "u", "предыдущий неотревьюенный файл"),
    b(Some('z'), "a", "список коммитов: свернуть/развернуть раунд"),
    b(Some('z'), "c", "список коммитов: свернуть раунд"),
    b(Some('z'), "o", "список коммитов: развернуть раунд"),
    b(Some(Z_TREE), "M", "дерево файлов: свернуть все каталоги"),
    b(Some(Z_TREE), "R", "дерево файлов: развернуть все каталоги"),
    b(
        Some(Z_TREE),
        "a",
        "дерево файлов: свернуть/развернуть каталог под курсором",
    ),
    b(Some(Z_TREE), "c", "дерево файлов: свернуть каталог"),
    b(Some(Z_TREE), "o", "дерево файлов: развернуть каталог"),
    b(Some(CTRL_W), "h", "окно слева (коммиты)"),
    b(Some(CTRL_W), "j", "окно ниже (коммиты → файлы)"),
    b(Some(CTRL_W), "k", "окно выше (файлы → коммиты)"),
    b(Some(CTRL_W), "l", "окно справа (дифф)"),
    b(Some(CTRL_W), "w", "следующее окно (коммиты → файлы → дифф)"),
    b(Some(CTRL_W), "W", "предыдущее окно"),
    b(
        Some(CTRL_W),
        "<",
        "сайдбар уже (:sidebar N; тянуть границу мышью)",
    ),
    b(Some(CTRL_W), ">", "сайдбар шире"),
    b(Some(CTRL_W), "=", "ширина сайдбара по умолчанию"),
];

/// Keys without a prefix, listed in the help only.
const PLAIN: &[(&str, &str)] = &[
    ("j/k", "курсор вниз/вверх (с числом: N строк)"),
    (
        "h/l",
        "дифф: сдвиг; в дереве файлов: свернуть/развернуть, l на файле: открыть",
    ),
    ("Ctrl-d/u", "полстраницы вниз/вверх"),
    ("Ctrl-f/b", "страница вниз/вверх"),
    ("Ctrl-e/y", "прокрутка без курсора"),
    ("G", "в конец файла (NG: к строке N)"),
    ("0", "горизонтальная прокрутка в начало"),
    ("s", "переключить side-by-side / inline"),
    ("a", "срез: весь диапазон"),
    ("v", "список коммитов: выделить срез"),
    ("Enter", "список коммитов: применить срез"),
    (
        "Ctrl-d/u",
        "список коммитов: на полвысоты списка вниз/вверх",
    ),
    ("/", "список коммитов: поиск по номеру, sha и тексту"),
    ("n/N", "список коммитов: следующее/предыдущее совпадение"),
    ("Enter", "дифф: развернуть свёрнутый механический файл"),
    (
        ":reset",
        "забыть все отметки отревьюено (комментарии остаются)",
    ),
    (":reload", "применить новые коммиты ветки"),
    ("c", "комментарий на строку (в VISUAL: на выделение)"),
    ("C", "комментарий на файл целиком"),
    ("v", "дифф: выделить строки для комментария или копирования"),
    (
        "y",
        "скопировать код выделения (VISUAL или мышь; Space y: с комментариями)",
    ),
    (
        "мышь",
        "клик, колесо; выделение копирует при отпускании; :set nomouse отдаёт мышь терминалу",
    ),
    ("x", "закрыть комментарий под курсором"),
    ("o", "вернуть комментарий в открытые"),
    (":comment", "комментарий на весь диапазон"),
    (
        "Y",
        "скопировать промпт из всех открытых комментариев (как Space p p)",
    ),
    (":prompt show", "показать промпт в оверлее"),
    (
        ":prompt save [путь]",
        "промпт в файл вместо буфера (тоже отправка)",
    ),
    (
        ":comments clear",
        "удалить все комментарии (с подтверждением)",
    ),
    (
        ":diagram",
        "диаграмма файла под курсором (:diagram here: фрагмента, :diagram group: группы)",
    ),
    (
        "h/l, j/k, y, q",
        "оверлей диаграммы: сдвиг, копировать Mermaid, закрыть",
    ),
    (
        "y",
        "саммари, объяснение, описание группы: копировать исходный Markdown",
    ),
    (
        ":effort, :model",
        "аналитик: effort (low..max) и модель со следующего запроса; без аргумента показать, default: сбросить",
    ),
    ("m", "файл отревьюено в срезе / снять отметку"),
    ("M", "отметить отревьюено и перейти к следующему"),
    ("Tab", "следующее окно: коммиты → файлы/группы → дифф"),
    ("S-Tab", "предыдущее окно"),
    (
        "j/k, Enter",
        "окно файлов: строки дерева; Enter открывает файл / сворачивает каталог",
    ),
    (
        "Ctrl-h/j/k/l",
        "окно слева/ниже/выше/справа (алиасы Ctrl-w)",
    ),
    (":", "командная строка (:scope, :theme light|dark|auto, :q)"),
    ("?", "эта справка"),
];

/// Notes shown at the top of the help.
const NOTES: &[&str] = &[
    "hjkl везде обычные движения и окон не переключают;",
    "окна переключаются через Ctrl-w и Tab.",
    "zellij перехватывает Ctrl-h/j/k/l (и Ctrl-w в некоторых раскладках):",
    "если алиасы не доходят, используйте Ctrl-w + h/j/k/l и Tab.",
    "Ctrl-h в большинстве терминалов приходит как Backspace и не работает.",
    "Длинный текст в оверлеях листается: j/k, ↑/↓, Ctrl-d/u, PageUp/PageDown,",
    "gg/G, Home/End и колесо мыши; позиция видна на нижней рамке.",
];

/// The `?` help: notes, then every key from [`PLAIN`] and [`BINDINGS`].
pub fn help_lines() -> Vec<String> {
    let mut lines: Vec<String> = NOTES.iter().map(|n| n.to_string()).collect();
    lines.push(String::new());
    for (key, label) in PLAIN {
        lines.push(format!("{key:<14} {label}"));
    }
    for x in BINDINGS {
        let chain = match x.prefix {
            Some(p @ (' ' | CTRL_W | SPACE_P)) => format!("{} {}", prefix_name(p), x.key),
            Some(p) => format!("{}{}", prefix_name(p), x.key),
            None => x.key.to_string(),
        };
        lines.push(format!("{chain:<14} {}", x.label));
    }
    lines
}

/// The title and rows of the which-key window for `prefix`.
pub fn which_key(prefix: char) -> (String, Vec<(&'static str, &'static str)>) {
    let rows = BINDINGS
        .iter()
        .filter(|x| x.prefix == Some(prefix))
        .map(|x| (x.key, x.label))
        .collect();
    (format!("which-key: {}", prefix_name(prefix)), rows)
}

/// What the bottom hint line describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Diff,
    Commits,
    /// The file list (Groups, when built).
    Files,
    /// The `Space f` file picker is open.
    FilePicker,
    /// A selection of commits is being extended.
    CommitsVisual,
    /// A prefix key is waiting for its continuation.
    Pending(char),
    /// `Space` waits inside the file picker: only [`PICKER_SPACE`] work.
    PickerSpace,
}

/// The bottom hint line: keys that make sense right now, chains spelled out.
pub fn hint_line(context: Context) -> String {
    match context {
        Context::Diff => {
            "j/k: строки  ]h/[h: хунки  Space: команды  Ctrl-w + h/j/k/l: окна  ?: справка  \
                          Ctrl-d/u: прокрутка  gg/G: начало/конец  ]f/[f: файлы  \
                          Ctrl-w </>/=: сайдбар"
                .into()
        }
        Context::Commits => "j/k: коммиты  Enter: срез  v: выделение  a: весь диапазон  \
                             /: поиск  n/N: совпадения  za: раунд  Ctrl-d/u: страница  \
                             Tab: окно  Space: команды  ?: справка"
            .into(),
        Context::Files => {
            "j/k: файлы и каталоги  h/l: свернуть/развернуть  zM/zR: свернуть/развернуть всё  \
                           Enter: открыть  \
                           ]g/[g: группы  Tab: окно  Space: команды  ?: справка"
                .into()
        }
        Context::FilePicker => {
            "j/k: строки  h/l: свернуть/развернуть  zM/zR: всё  Enter: открыть  \
                                gg/G: начало/конец  Space g: группы/дерево  Esc: закрыть"
                .into()
        }
        Context::CommitsVisual => {
            "-- VISUAL --  j/k: расширить  Enter: срез  v: снять  Esc: отмена".into()
        }
        Context::Pending(CTRL_W) => {
            "Ctrl-w + h/j/k/l: окна  Ctrl-w + w/W: следующее/предыдущее окно  \
                Ctrl-w + </>: ширина сайдбара  Ctrl-w + =: сброс  Esc: отмена"
                .into()
        }
        Context::PickerSpace => BINDINGS
            .iter()
            .filter(|x| x.prefix == Some(' ') && PICKER_SPACE.contains(&x.key))
            .map(|x| format!("Space + {}: {}", x.key, x.label))
            .chain(["Esc: отмена".to_string()])
            .collect::<Vec<_>>()
            .join("  "),
        Context::Pending(prefix) => {
            let name = prefix_name(prefix);
            let mut parts: Vec<String> = BINDINGS
                .iter()
                .filter(|x| x.prefix == Some(prefix))
                .map(|x| format!("{name} + {}: {}", x.key, x.label))
                .collect();
            parts.push("Esc: отмена".into());
            parts.join("  ")
        }
    }
}

pub fn prefix_name(prefix: char) -> String {
    match prefix {
        ' ' => "Space".into(),
        CTRL_W => "Ctrl-w".into(),
        SPACE_P => "Space p".into(),
        Z_TREE => "z".into(),
        c => c.to_string(),
    }
}
