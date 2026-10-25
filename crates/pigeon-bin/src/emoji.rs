//! The composer's emoji picker set: a few hundred common emoji with names to
//! search by, in picker sections. Everything else is one ⌃⌘Space away (the
//! macOS Character Viewer works in the editor as is).

/// (section, emoji and their names). One emoji per line: the emoji, then
/// the words it's found by.
pub const SECTIONS: &[(&str, &str)] = &[
    (
        "Smileys",
        "😀 grinning smile happy
😃 smiley big smile happy
😄 smile grin happy
😁 beaming grin
😆 laughing squint
😅 sweat smile relief
🤣 rolling on the floor laughing rofl
😂 joy tears of joy laughing lol
🙂 slightly smiling
🙃 upside down
😉 wink
😊 blush smiling eyes
😇 innocent halo angel
🥰 smiling hearts love
😍 heart eyes love
🤩 star struck excited
😘 kiss blowing
😗 kissing
😚 kissing closed eyes
😋 yum delicious
😛 tongue
😜 winking tongue crazy
🤪 zany crazy goofy
😝 squinting tongue
🤑 money mouth
🤗 hugging hug
🤭 hand over mouth oops
🤫 shushing quiet secret
🤔 thinking hmm
🤐 zipper mouth
🤨 raised eyebrow skeptical
😐 neutral
😑 expressionless
😶 no mouth speechless
😏 smirk
😒 unamused
🙄 eye roll
😬 grimacing awkward
😮‍💨 exhaling sigh relief
🤥 lying
😌 relieved
😔 pensive sad
😪 sleepy
🤤 drooling
😴 sleeping tired zzz
😷 mask sick
🤒 thermometer sick ill
🤕 bandage hurt
🤢 nauseated sick
🤮 vomiting
🤧 sneezing cold
🥵 hot sweating
🥶 cold freezing
🥴 woozy dizzy
😵 dizzy
🤯 exploding head mind blown
🤠 cowboy
🥳 partying party celebrate
😎 cool sunglasses
🤓 nerd geek
🧐 monocle
😕 confused
😟 worried
🙁 slightly frowning sad
😮 open mouth surprised wow
😯 hushed surprised
😲 astonished shocked
😳 flushed embarrassed
🥺 pleading puppy eyes
😦 frowning open mouth
😧 anguished
😨 fearful scared
😰 anxious sweat
😥 sad relieved disappointed
😢 crying sad tear
😭 sobbing loudly crying
😱 screaming fear
😖 confounded
😣 persevering
😞 disappointed
😓 downcast sweat
😩 weary
😫 tired
🥱 yawning bored
😤 triumph huffing
😡 pouting angry mad
😠 angry
🤬 cursing swearing
😈 smiling devil
💀 skull dead
💩 poo
🤡 clown
👻 ghost
👽 alien
🤖 robot
😺 smiling cat
😹 cat tears of joy
😻 cat heart eyes
🙈 see no evil monkey
🙉 hear no evil monkey
🙊 speak no evil monkey",
    ),
    (
        "People",
        "👋 wave hello hi bye
🤚 raised back of hand
✋ raised hand high five stop
🖖 vulcan salute
👌 ok perfect
🤌 pinched fingers
🤏 pinching small
✌️ victory peace
🤞 crossed fingers luck hope
🤟 love you gesture
🤘 rock on horns
🤙 call me hang loose
👈 point left
👉 point right
👆 point up
👇 point down
☝️ index up
👍 thumbs up yes like approve
👎 thumbs down no dislike
✊ raised fist
👊 fist bump punch
👏 clapping applause bravo
🙌 raising hands celebrate hooray
👐 open hands
🤲 palms up
🤝 handshake deal agreement
🙏 folded hands please thanks pray
✍️ writing
💪 flexed biceps strong
🧠 brain smart
👀 eyes looking watching
👁️ eye
👄 mouth
🫶 heart hands
🫡 salute
🫠 melting
🫢 hand over mouth gasp
🤷 shrug dunno
🤦 facepalm
🙋 raising hand question
🙆 ok gesture
🙅 no gesture
💁 tipping hand
🙇 bowing sorry
🧑‍💻 technologist developer coder laptop
👩‍💻 woman technologist
👨‍💻 man technologist
🧑‍🏫 teacher
🧑‍⚕️ health worker doctor
🧑‍🍳 cook chef
👶 baby
🧒 child
👪 family
🏃 running
🚶 walking
💃 dancing
🕺 man dancing
👯 party people
🧘 yoga meditation",
    ),
    (
        "Hearts & symbols",
        "❤️ red heart love
🧡 orange heart
💛 yellow heart
💚 green heart
💙 blue heart
💜 purple heart
🖤 black heart
🤍 white heart
🤎 brown heart
💔 broken heart
❤️‍🔥 heart on fire
💕 two hearts
💞 revolving hearts
💖 sparkling heart
💗 growing heart
💘 heart arrow cupid
💝 heart ribbon gift
💯 hundred perfect 100
✅ check mark done yes
☑️ check box
✔️ check
❌ cross no wrong
❎ cross mark button
➕ plus add
➖ minus
❗ exclamation important
❓ question
⚠️ warning caution
⛔ no entry
🚫 prohibited forbidden
🔴 red circle
🟠 orange circle
🟡 yellow circle
🟢 green circle
🔵 blue circle
🟣 purple circle
⚫ black circle
⚪ white circle
⭐ star
🌟 glowing star
✨ sparkles magic
⚡ zap lightning
🔥 fire hot lit
💥 boom collision
💫 dizzy star
💬 speech balloon comment
💭 thought balloon
🗯️ anger bubble
💤 zzz sleep
🎵 music note
🎶 notes music
➡️ right arrow
⬅️ left arrow
⬆️ up arrow
⬇️ down arrow
🔁 repeat
🔄 refresh sync
🆗 ok button
🆕 new
🆓 free
🔝 top
🆘 sos help
♻️ recycle
💲 dollar sign
™️ trademark
©️ copyright
®️ registered",
    ),
    (
        "Nature & food",
        "🐶 dog puppy
🐱 cat kitten
🐭 mouse
🐰 rabbit bunny
🦊 fox
🐻 bear
🐼 panda
🐨 koala
🐯 tiger
🦁 lion
🐮 cow
🐷 pig
🐸 frog
🐵 monkey
🐔 chicken
🐧 penguin
🐦 bird
🕊️ dove peace pigeon
🦉 owl
🦄 unicorn
🐝 bee
🦋 butterfly
🐢 turtle
🐍 snake
🐙 octopus
🐬 dolphin
🐳 whale
🦈 shark
🌸 cherry blossom flower
🌹 rose
🌻 sunflower
🌷 tulip
🌱 seedling growth
🌲 evergreen tree
🌴 palm tree
🍀 four leaf clover luck
🍁 maple leaf autumn
☀️ sun sunny
🌤️ sun behind cloud
⛅ partly cloudy
☁️ cloud
🌧️ rain
⛈️ storm thunder
❄️ snowflake snow cold
🌈 rainbow
🌙 moon night
🌍 earth globe world
🌊 wave ocean sea
🍎 apple
🍊 orange tangerine
🍋 lemon
🍌 banana
🍉 watermelon
🍇 grapes
🍓 strawberry
🍑 peach
🍍 pineapple
🥑 avocado
🥕 carrot
🌽 corn
🌶️ hot pepper spicy
🍞 bread
🧀 cheese
🥚 egg
🥞 pancakes
🍔 burger hamburger
🍟 fries
🍕 pizza
🌮 taco
🌯 burrito
🥙 pita falafel
🍣 sushi
🍜 noodles ramen
🥗 salad
🍿 popcorn
🍩 doughnut donut
🍪 cookie
🎂 birthday cake
🍰 cake shortcake
🧁 cupcake
🍫 chocolate
🍬 candy
☕ coffee hot beverage tea
🍵 tea
🧃 juice
🥤 soda drink
🍺 beer
🍻 cheers beers
🥂 clinking glasses toast cheers
🍷 wine
🍸 cocktail
🍾 champagne celebrate",
    ),
    (
        "Activities & travel",
        "🎉 party popper tada celebrate congrats
🎊 confetti
🎈 balloon
🎁 gift present
🏆 trophy winner
🥇 gold medal first
🥈 silver medal second
🥉 bronze medal third
🏅 medal
⚽ soccer football
🏀 basketball
🏈 american football
⚾ baseball
🎾 tennis
🏐 volleyball
🏓 ping pong
🏸 badminton
⛳ golf
🏊 swimming
🚴 cycling bike
🎯 bullseye target goal
🎮 video game
🎲 dice game
🧩 puzzle
🎨 art palette
🎬 clapper movie film
🎤 microphone sing
🎧 headphones
🎸 guitar
🎹 piano
🚗 car
🚕 taxi
🚌 bus
🚆 train
🚇 metro subway
✈️ airplane flight travel
🛫 departure
🛬 arrival
🚀 rocket launch ship it
🚲 bicycle
🛴 scooter
⛵ sailboat
🚢 ship
🏠 house home
🏢 office building
🏥 hospital
🏫 school
⛪ church
🕌 mosque
🕍 synagogue
🗽 statue of liberty
🗼 tower
🏖️ beach vacation
🏝️ island
⛰️ mountain
🏕️ camping
🗺️ map
🧭 compass
⛱️ umbrella beach",
    ),
    (
        "Objects",
        "💡 bulb idea
📌 pushpin pin
📍 round pin location
📎 paperclip attachment
🔗 link
📝 memo note write
✏️ pencil
🖊️ pen
📄 page document
📃 page curl
📑 bookmark tabs
📊 bar chart stats
📈 chart up increasing growth
📉 chart down decreasing
📅 calendar date
📆 tear off calendar
🗓️ spiral calendar
📋 clipboard
📁 folder
📂 open folder
🗂️ dividers
🗃️ card box
🗑️ wastebasket trash
📦 package box shipping
📫 mailbox
📧 email e-mail
✉️ envelope letter
📨 incoming envelope
📩 envelope arrow
📮 postbox
📞 telephone phone call
📱 mobile phone
💻 laptop computer
🖥️ desktop computer
⌨️ keyboard
🖨️ printer
🖱️ mouse
💾 floppy disk save
💿 disc
📷 camera photo
📸 camera flash
🎥 movie camera
📺 television tv
📻 radio
⏰ alarm clock
⏱️ stopwatch
⏳ hourglass waiting
⌛ hourglass done
🕐 one o'clock time
🔋 battery
🔌 plug
🔍 magnifying glass search
🔎 search right
🔒 locked lock secure
🔓 unlocked
🔑 key
🗝️ old key
🔨 hammer
🛠️ tools
🔧 wrench fix
⚙️ gear settings
🧰 toolbox
🧪 test tube experiment
🔬 microscope
💊 pill medicine
💉 syringe vaccine
🩺 stethoscope
🧹 broom
🛒 shopping cart
💰 money bag
💵 dollar banknote cash
💳 credit card
🧾 receipt
💼 briefcase work
🎒 backpack
👓 glasses
👔 necktie shirt
👗 dress
👟 sneaker shoe
👑 crown king queen
💍 ring
💎 gem diamond
📚 books
📖 open book read
📰 newspaper news
🔖 bookmark
🏷️ label tag
🧸 teddy bear
🪄 magic wand
🔔 bell notification
🔕 bell muted
📣 megaphone announcement
📢 loudspeaker
🚩 flag
🏁 checkered flag finish
🏳️ white flag",
    ),
];

/// Shown under "Frequently used" until the user has picked some.
pub const STARTERS: &str = "👍 🙂 😂 ❤️ 🙏 🎉 👀 ✅ 🔥 😊 👏 😅 🤔 💯 🙌 😉";

/// The set as JSON for the picker script: [[section, [[emoji, names], …]], …].
pub fn json() -> String {
    let sections: Vec<(&str, Vec<(&str, &str)>)> = SECTIONS
        .iter()
        .map(|(name, lines)| (*name, lines.lines().filter_map(|l| l.trim().split_once(' ')).collect()))
        .collect();
    serde_json::to_string(&(sections, STARTERS.split(' ').collect::<Vec<_>>())).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emoji_has_a_name_and_appears_once() {
        let mut seen = std::collections::HashSet::new();
        for (_, lines) in SECTIONS {
            for l in lines.lines() {
                let (e, names) = l.trim().split_once(' ').expect("emoji then names");
                assert!(!names.trim().is_empty() && !e.is_ascii(), "{l:?}");
                assert!(seen.insert(e), "{e} is listed twice");
            }
        }
        assert!(seen.len() > 300, "{} emoji", seen.len());
        for s in STARTERS.split(' ') {
            assert!(seen.contains(s), "starter {s} isn't in the set");
        }
        let j = json();
        assert!(j.starts_with("[[[\"Smileys\",[[\"😀\",\"grinning smile happy\"]"), "{}", &j[..60]);
    }
}
