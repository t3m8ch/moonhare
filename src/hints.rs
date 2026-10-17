//! The keymap as data: which-key lists, the `?` help and the bottom hint line
//! are all derived from [`BINDINGS`]. A feature that adds a key adds a row here
//! and it shows up everywhere.

use std::time::Duration;

/// How long a pending prefix waits before the which-key window opens.
pub const WHICH_KEY_DELAY: Duration = Duration::from_millis(300);

/// The internal name of the `Ctrl-w` prefix.
pub const CTRL_W: char = '\u{17}';

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
    b(Some(' '), "c", "выбор коммита (срез)"),
    b(Some(' '), "e", "показать/скрыть сайдбар"),
    b(Some(' '), "f", "выбор файла"),
    b(Some(' '), "g", "сайдбар: группы / дерево каталогов"),
    b(Some(' '), "s", "саммари аналитика"),
    b(Some(' '), "u", "фильтр: только непросмотренные файлы"),
    b(Some(' '), "x", "аналитик: объяснить хунк под курсором"),
    b(Some(' '), "X", "аналитик: объяснить файл под курсором"),
    b(Some('g'), "g", "в начало файла (Ngg: к строке N)"),
    b(Some(']'), "c", "следующий коммит"),
    b(Some(']'), "f", "следующий файл"),
    b(Some(']'), "h", "следующий хунк"),
    b(Some(']'), "u", "следующий неотревьюенный файл"),
    b(Some('['), "c", "предыдущий коммит"),
    b(Some('['), "f", "предыдущий файл"),
    b(Some('['), "h", "предыдущий хунк"),
    b(Some('['), "u", "предыдущий неотревьюенный файл"),
    b(Some(CTRL_W), "h", "окно слева (коммиты)"),
    b(Some(CTRL_W), "j", "окно ниже (дифф)"),
    b(Some(CTRL_W), "k", "окно выше (коммиты)"),
    b(Some(CTRL_W), "l", "окно справа (дифф)"),
    b(Some(CTRL_W), "w", "следующее окно"),
];

/// Keys without a prefix, listed in the help only.
const PLAIN: &[(&str, &str)] = &[
    ("j/k", "курсор вниз/вверх (с числом: N строк)"),
    (
        "h/l",
        "дифф: сдвиг по горизонтали; в списках: свернуть группу / открыть файл",
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
    ("Enter", "дифф: развернуть свёрнутый механический файл"),
    (
        ":reset",
        "забыть все отметки отревьюено (комментарии остаются)",
    ),
    ("c", "комментарий на строку (в VISUAL: на выделение)"),
    ("C", "комментарий на файл целиком"),
    ("v", "дифф: выделить строки для комментария"),
    ("x", "закрыть комментарий под курсором"),
    ("o", "вернуть комментарий в открытые"),
    (":comment", "комментарий на весь диапазон"),
    (
        ":comments clear",
        "удалить все комментарии (с подтверждением)",
    ),
    ("m", "файл отревьюено в срезе / снять отметку"),
    ("M", "отметить отревьюено и перейти к следующему"),
    ("Tab", "следующее окно"),
    (
        "Ctrl-h/j/k/l",
        "окно слева/ниже/выше/справа (алиасы Ctrl-w)",
    ),
    (":", "командная строка (:scope, :q)"),
    ("?", "эта справка"),
];

/// Notes shown at the top of the help.
const NOTES: &[&str] = &[
    "hjkl везде обычные движения и окон не переключают;",
    "окна переключаются через Ctrl-w и Tab.",
    "zellij перехватывает Ctrl-h/j/k/l (и Ctrl-w в некоторых раскладках):",
    "если алиасы не доходят, используйте Ctrl-w + h/j/k/l и Tab.",
    "Ctrl-h в большинстве терминалов приходит как Backspace и не работает.",
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
            Some(p @ (' ' | CTRL_W)) => format!("{} {}", prefix_name(p), x.key),
            Some(p) => format!("{p}{}", x.key),
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
    /// A selection of commits is being extended.
    CommitsVisual,
    /// A prefix key is waiting for its continuation.
    Pending(char),
}

/// The bottom hint line: keys that make sense right now, chains spelled out.
pub fn hint_line(context: Context) -> String {
    match context {
        Context::Diff => {
            "j/k: строки  ]h/[h: хунки  Space: команды  Ctrl-w + h/j/k/l: окна  ?: справка  \
                          Ctrl-d/u: прокрутка  gg/G: начало/конец  ]f/[f: файлы"
                .into()
        }
        Context::Commits => "j/k: коммиты  Enter: срез  v: выделение  a: весь диапазон  \
                             Tab: окно  Space: команды  Ctrl-w + h/j/k/l: окна  ?: справка"
            .into(),
        Context::CommitsVisual => {
            "-- VISUAL --  j/k: расширить  Enter: срез  v: снять  Esc: отмена".into()
        }
        Context::Pending(CTRL_W) => {
            "Ctrl-w + h/j/k/l: окна  Ctrl-w + w: следующее окно  Esc: отмена".into()
        }
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
        c => c.to_string(),
    }
}
