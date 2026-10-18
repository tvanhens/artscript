#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Word {
    Use,
    As,
    From,
    Palette,
    Style,
    Finish,
    Def,
    Rule,
    Let,
    Light,
    Brush,
    Assert,
    Expect,
    If,
    Then,
    Else,
    Each,
    In,
    And,
    Or,
    Not,
    True,
    False,
    None,
    Fx,
    Mask,
    Slot,
    Ramp,
    Color,
    Sksl,
    Field,
    Warp,
    Backdrop,
    Profile,
    Pose,
    Joint,
    Ease,
    T,
    Frames,
    Anim,
    Play,
    Key,
    Clip,
    Target,
    Loop,
    Set,
    Mark,
    Root,
    Visible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Keyword,
    ReservedName,
    PaletteHead,
    /// A declaration head only when it starts a line. Everywhere else it is an ordinary name,
    /// so `brush:` is a key and `brush` a parameter.
    DeclHead,
}

const WORDS: &[(Word, &str, Class)] = &[
    (Word::Use, "use", Class::Keyword),
    (Word::As, "as", Class::Keyword),
    (Word::From, "from", Class::Keyword),
    (Word::Palette, "palette", Class::Keyword),
    (Word::Style, "style", Class::Keyword),
    (Word::Finish, "finish", Class::Keyword),
    (Word::Def, "def", Class::Keyword),
    (Word::Rule, "rule", Class::Keyword),
    (Word::Let, "let", Class::Keyword),
    (Word::Light, "light", Class::Keyword),
    (Word::Brush, "brush", Class::DeclHead),
    (Word::Assert, "assert", Class::Keyword),
    (Word::Expect, "expect", Class::Keyword),
    (Word::If, "if", Class::Keyword),
    (Word::Then, "then", Class::Keyword),
    (Word::Else, "else", Class::Keyword),
    (Word::Each, "each", Class::Keyword),
    (Word::In, "in", Class::Keyword),
    (Word::And, "and", Class::Keyword),
    (Word::Or, "or", Class::Keyword),
    (Word::Not, "not", Class::Keyword),
    (Word::True, "true", Class::Keyword),
    (Word::False, "false", Class::Keyword),
    (Word::None, "none", Class::Keyword),
    (Word::Fx, "fx", Class::Keyword),
    (Word::Mask, "mask", Class::Keyword),
    (Word::Slot, "slot", Class::Keyword),
    (Word::Ramp, "ramp", Class::PaletteHead),
    (Word::Color, "color", Class::PaletteHead),
    (Word::Sksl, "sksl", Class::ReservedName),
    (Word::Field, "field", Class::ReservedName),
    (Word::Warp, "warp", Class::ReservedName),
    (Word::Backdrop, "backdrop", Class::ReservedName),
    (Word::Profile, "profile", Class::ReservedName),
    (Word::Pose, "pose", Class::ReservedName),
    (Word::Joint, "joint", Class::ReservedName),
    (Word::Ease, "ease", Class::ReservedName),
    (Word::T, "t", Class::ReservedName),
    (Word::Frames, "frames", Class::ReservedName),
    (Word::Anim, "anim", Class::Keyword),
    (Word::Play, "play", Class::Keyword),
    (Word::Key, "key", Class::ReservedName),
    (Word::Clip, "clip", Class::ReservedName),
    (Word::Target, "target", Class::ReservedName),
    (Word::Loop, "loop", Class::ReservedName),
    (Word::Set, "set", Class::ReservedName),
    (Word::Mark, "mark", Class::ReservedName),
    (Word::Root, "root", Class::ReservedName),
    (Word::Visible, "visible", Class::ReservedName),
];

impl Word {
    pub(crate) fn lookup(text: &str) -> Option<Word> {
        WORDS.iter().find(|row| row.1 == text).map(|row| row.0)
    }

    fn row(self) -> &'static (Word, &'static str, Class) {
        WORDS
            .iter()
            .find(|row| row.0 == self)
            .expect("every word has a row")
    }

    pub(crate) fn as_str(self) -> &'static str {
        self.row().1
    }

    pub(crate) fn class(self) -> Class {
        self.row().2
    }

    pub(crate) fn reserved(self) -> bool {
        matches!(self.class(), Class::Keyword | Class::ReservedName)
    }
}

pub(crate) fn keyword(text: &str) -> Option<Word> {
    Word::lookup(text).filter(|w| w.class() == Class::Keyword)
}

pub(crate) fn is_reserved(text: &str) -> bool {
    Word::lookup(text).is_some_and(Word::reserved)
}

/// A name that opens a declaration when it is a line's first word.
pub(crate) fn is_decl_head(text: &str) -> bool {
    Word::lookup(text).is_some_and(|w| w.class() == Class::DeclHead)
}
