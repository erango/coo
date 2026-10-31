//! The daily hello's scenes (greeting.rs picks one): the app's pigeon
//! (bird.rs) with a prop or two, on a 200×125 sky. Colors come from the
//! theme: the scarf and props use the accent, beak and feet the warning
//! color. Loops are `greet-*` classes in app.css.

use crate::greeting::Scene;

/// The scene's description (for screen readers) and its SVG body.
pub fn art(scene: Scene) -> (&'static str, &'static str) {
    match scene {
        Scene::Coffee => (
            "The pigeon under a blanket with a mug of coffee at sunrise",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F6C76B 30%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="164" cy="100" r="16" fill="#F6B94C" opacity="0.9"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<rect x="60" y="82" width="20" height="18" rx="3" fill="var(--accent, #0A7A64)"></rect>
<path d="M60 86C54 86 54 95 60 95" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="2.5"></path>
<ellipse cx="70" cy="82.6" rx="9" ry="1.5" fill="#6B4226"></ellipse>
<path class="greet-steam" d="M66 78C64 74 68 72 66 68" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1.4" stroke-linecap="round"></path>
<path class="greet-steam2" d="M73 78C71 74 75 72 73 68" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1.4" stroke-linecap="round"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M24 30C34 28 48 31 59 36L58 39C50 43 40 46 30 45C24 44.6 19 42 16 38C18 33 20 31 24 30Z" fill="color-mix(in srgb, var(--accent, #0A7A64) 35%, #FFFFFF)"></path>
<path d="M30 31L33 44M40 31.5L42 44.5M50 33.5L51 42" stroke="color-mix(in srgb, var(--accent, #0A7A64) 65%, #FFFFFF)" stroke-width="1.4"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<path d="M17.6 14.4Q19.5 16 21.4 14.4" fill="none" stroke="#1F3A5C" stroke-width="1.2" stroke-linecap="round"></path>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::LateMorning => (
            "The pigeon with a croissant on a sunny morning",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #8EC5E8 26%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="158" cy="30" r="11" fill="#F6C24C"></circle>
<path d="M30 34C30 30 34 28 37 30C38 26 44 26 45 30C48 30 50 32 49 35H31Z" fill="#FFFFFF" opacity="0.9"></path>
<path d="M120 22C120 19 123 17.5 125.5 19C126.5 16 131 16 131.8 19C134 19 135.5 20.5 135 23H121Z" fill="#FFFFFF" opacity="0.8"></path>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<ellipse cx="68" cy="99" rx="17" ry="2.6" fill="#FFFFFF" stroke="var(--line-strong, rgba(15,17,20,0.15))"></ellipse>
<path d="M55 96C57 87 79 87 81 96C77 92.5 73 91.5 68 91.5C63 91.5 59 92.5 55 96Z" fill="#E2A456" stroke="#B8742C" stroke-width="0.8"></path>
<path d="M62 89.6L63.5 94M68 88.8V93.5M74 89.6L72.5 94" stroke="#B8742C" stroke-width="0.8" fill="none"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Afternoon => (
            "The pigeon pecking at a sandwich at lunchtime",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #8EC5E8 26%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="40" cy="24" r="10" fill="#F6C24C"></circle>
<path d="M140 30C140 26 144 24 147 26C148 22 154 22 155 26C158 26 160 28 159 31H141Z" fill="#FFFFFF" opacity="0.9"></path>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<path d="M56 99L72 84L80 99Z" fill="#F1D19A" stroke="#C99A52" stroke-width="0.8"></path>
<path d="M60 96L73 87.5" stroke="#6DBE7B" stroke-width="2" stroke-linecap="round"></path>
<path d="M62 98L74 89.5" stroke="#E2614A" stroke-width="1.4" stroke-linecap="round"></path>
<circle cx="84" cy="99.4" r="0.9" fill="#C99A52"></circle>
<circle cx="87.5" cy="99" r="0.8" fill="#C99A52"></circle>
<circle cx="90.5" cy="99.5" r="0.9" fill="#C99A52"></circle>
<g transform="translate(86 46) rotate(-12 30 54)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Evening => (
            "The pigeon with a cup of tea at sunset",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F09A6A 30%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="166" cy="100" r="18" fill="#F08A5D"></circle>
<circle class="greet-twinkle" cx="40" cy="22" r="1.2" fill="#FFFFFF"></circle>
<circle class="greet-twinkle2" cx="70" cy="14" r="1" fill="#FFFFFF"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 8%, var(--bg-0, #FFFFFF))"></path>
<ellipse cx="68" cy="99.5" rx="14" ry="2" fill="#FFFFFF" stroke="var(--line-strong, rgba(15,17,20,0.15))"></ellipse>
<path d="M78 88C83 88 83 94 77 94" fill="none" stroke="var(--line-strong, rgba(15,17,20,0.3))" stroke-width="1.6"></path>
<path d="M58 86H78L76 96C75.5 98 74 99 72 99H64C62 99 60.5 98 60 96Z" fill="#FFFFFF" stroke="var(--line-strong, rgba(15,17,20,0.15))"></path>
<ellipse cx="68" cy="86.4" rx="9.6" ry="1.4" fill="#C77B3B"></ellipse>
<path d="M63 86L61.5 92" stroke="#A3A6AE" stroke-width="0.6"></path>
<rect x="60" y="92" width="3.2" height="3.6" rx="0.5" fill="var(--accent, #0A7A64)"></rect>
<path class="greet-steam" d="M65 82C63 78 67 76 65 72" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1.4" stroke-linecap="round"></path>
<path class="greet-steam2" d="M71 82C69 78 73 76 71 72" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1.4" stroke-linecap="round"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Night => (
            "The pigeon in sunglasses under a disco ball, city lights behind",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="#1B2340"></rect>
<path d="M172 12A11 11 0 1 0 182 30A9 9 0 1 1 172 12Z" fill="#F4E7B5"></path>
<rect x="0" y="70" width="22" height="30" fill="#252E52"></rect>
<rect x="24" y="58" width="18" height="42" fill="#252E52"></rect>
<rect x="150" y="62" width="20" height="38" fill="#252E52"></rect>
<rect x="172" y="74" width="28" height="26" fill="#252E52"></rect>
<rect x="5" y="76" width="3" height="3" fill="#F6C453"></rect>
<rect x="13" y="84" width="3" height="3" fill="#F6C453"></rect>
<rect x="28" y="64" width="3" height="3" fill="#F6C453"></rect>
<rect x="35" y="76" width="3" height="3" fill="#F6C453"></rect>
<rect x="155" y="68" width="3" height="3" fill="#F6C453"></rect>
<rect x="163" y="80" width="3" height="3" fill="#F6C453"></rect>
<rect x="180" y="80" width="3" height="3" fill="#F6C453"></rect>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="#131A30"></path>
<ellipse cx="118" cy="101" rx="32" ry="3" fill="var(--accent, #3CCFAE)" opacity="0.35"></ellipse>
<path d="M66 0V16" stroke="#8A93A6" stroke-width="0.8"></path>
<g class="greet-disco">
<circle cx="66" cy="26" r="10" fill="#C9D2DE"></circle>
<path d="M56 26H76M58 21H74M58 31H74M66 16V36M61 17C59 22 59 30 61 35M71 17C73 22 73 30 71 35" fill="none" stroke="#8A93A6" stroke-width="0.6"></path>
<rect x="60" y="19" width="4" height="3" fill="#FFFFFF" opacity="0.9"></rect>
</g>
<path class="greet-twinkle" d="M44 42L45 45L48 46L45 47L44 50L43 47L40 46L43 45Z" fill="#FFFFFF"></path>
<path class="greet-twinkle2" d="M88 34L88.8 36.2L91 37L88.8 37.8L88 40L87.2 37.8L85 37L87.2 36.2Z" fill="var(--accent, #3CCFAE)"></path>
<path class="greet-twinkle2" d="M110 14L110.8 16.2L113 17L110.8 17.8L110 20L109.2 17.8L107 17L109.2 16.2Z" fill="#F6C453"></path>
<g class="greet-z">
<path d="M80 60V51L86 49.5V58" fill="none" stroke="var(--accent, #3CCFAE)" stroke-width="1.2"></path>
<circle cx="78.6" cy="60" r="1.8" fill="var(--accent, #3CCFAE)"></circle>
<circle cx="84.6" cy="58" r="1.8" fill="var(--accent, #3CCFAE)"></circle>
</g>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="rgba(255,255,255,0.2)" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #3CCFAE)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #F2B44B)"></path>
<path d="M15.5 12.4H24V15C24 16.6 22.8 17.2 21.4 17.2H18.2C16.6 17.2 15.5 16.6 15.5 15Z" fill="#16171A"></path>
<path d="M24 13H30" stroke="#16171A" stroke-width="0.9"></path>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #F2B44B)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::SmallHours => (
            "The pigeon asleep in a nightcap under the moon",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="#1B2340"></rect>
<path d="M40 14A11 11 0 1 0 50 32A9 9 0 1 1 40 14Z" fill="#F4E7B5"></path>
<circle class="greet-twinkle" cx="80" cy="20" r="1.1" fill="#FFFFFF"></circle>
<circle class="greet-twinkle2" cx="150" cy="16" r="1.3" fill="#FFFFFF"></circle>
<circle class="greet-twinkle" cx="176" cy="40" r="1" fill="#FFFFFF"></circle>
<circle class="greet-twinkle2" cx="20" cy="50" r="1" fill="#FFFFFF"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="#131A30"></path>
<text class="greet-z" x="82" y="58" font-size="9" font-weight="700" fill="#C9D2DE" font-family="Inter, sans-serif">z</text>
<text class="greet-z2" x="72" y="46" font-size="12" font-weight="700" fill="#C9D2DE" font-family="Inter, sans-serif">Z</text>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="rgba(255,255,255,0.2)" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #3CCFAE)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #F2B44B)"></path>
<path d="M17.6 14.4Q19.5 16 21.4 14.4" fill="none" stroke="#1F3A5C" stroke-width="1.2" stroke-linecap="round"></path>
<path d="M13 12.5C14.5 6 23 3.5 28.5 7.5C32 10 35 12.5 37.5 16C33.5 14.5 30.5 13.2 27.5 12Z" fill="color-mix(in srgb, var(--accent, #3CCFAE) 70%, #ffffff)" stroke="color-mix(in srgb, var(--accent, #3CCFAE) 80%, #000000)" stroke-width="0.8"></path>
<path d="M13 12.5C17 10.8 23 10.6 27.5 12" fill="none" stroke="#FFFFFF" stroke-width="1.6" stroke-linecap="round"></path>
<circle cx="38.2" cy="16.6" r="2.2" fill="#FFFFFF" stroke="color-mix(in srgb, var(--accent, #3CCFAE) 80%, #000000)" stroke-width="0.8"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #F2B44B)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::FirstWorkday => (
            "The pigeon with a fresh to-do list",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #8EC5E8 26%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="164" cy="90" r="12" fill="#F6C24C"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<rect x="54" y="70" width="26" height="30" rx="2.5" fill="#C9A36B"></rect>
<rect x="57" y="74" width="20" height="24" fill="#FFFFFF"></rect>
<rect x="62" y="68" width="10" height="5" rx="1.5" fill="#8A93A6"></rect>
<path d="M59 79L60.6 80.6L63 78M59 85L60.6 86.6L63 84" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"></path>
<path d="M65 79.5H74M65 85.5H73M65 91.5H72" stroke="#A3A6AE" stroke-width="1.2" stroke-linecap="round"></path>
<rect x="59" y="89.8" width="3.4" height="3.4" rx="0.6" fill="none" stroke="#A3A6AE" stroke-width="0.9"></rect>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::LastWorkday => (
            "The pigeon in sunglasses under party bunting",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, var(--accent, #0A7A64) 12%, var(--bg-0, #FFFFFF))"></rect>
<path d="M0 14Q100 54 200 14" fill="none" stroke="#8A93A6" stroke-width="1"></path>
<path d="M13 17.6H27L20 30Z" fill="var(--accent, #0A7A64)"></path>
<path d="M43 21.5H57L50 34Z" fill="#F2B44B"></path>
<path d="M73 23.6H87L80 36Z" fill="#8EC5E8"></path>
<path d="M103 23.9H117L110 36Z" fill="#F08A5D"></path>
<path d="M133 22.4H147L140 35Z" fill="var(--accent, #0A7A64)"></path>
<path d="M163 19.1H177L170 31Z" fill="#F2B44B"></path>
<rect class="greet-fall" x="40" y="46" width="3" height="5" fill="#F08A5D"></rect>
<rect class="greet-fall2" x="160" y="44" width="3" height="5" fill="var(--accent, #0A7A64)"></rect>
<rect class="greet-fall" x="70" y="52" width="3" height="5" fill="#8EC5E8"></rect>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<path d="M15.5 12.4H24V15C24 16.6 22.8 17.2 21.4 17.2H18.2C16.6 17.2 15.5 16.6 15.5 15Z" fill="#16171A"></path>
<path d="M24 13H30" stroke="#16171A" stroke-width="0.9"></path>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::WeekendFirst => (
            "The pigeon in sunglasses flying a kite on the grass",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #8EC5E8 26%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="168" cy="24" r="10" fill="#F6C24C"></circle>
<path d="M120 36C120 32 124 30 127 32C128 28 134 28 135 32C138 32 140 34 139 37H121Z" fill="#FFFFFF" opacity="0.9"></path>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, #6DBE7B 32%, var(--bg-0, #FFFFFF))"></path>
<path d="M48 40C66 56 82 58 94 62" fill="none" stroke="#8A93A6" stroke-width="0.7"></path>
<g class="greet-sway">
<path d="M48 14L58 26L48 40L38 26Z" fill="#F08A5D"></path>
<path d="M48 14V40M38 26H58" stroke="#FFFFFF" stroke-width="0.8"></path>
<path d="M48 40C44 46 52 50 47 56" fill="none" stroke="#F08A5D" stroke-width="1.4"></path>
<path d="M45 46L50 47M45 52L50 53" stroke="var(--accent, #0A7A64)" stroke-width="1.6" stroke-linecap="round"></path>
</g>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<path d="M15.5 12.4H24V15C24 16.6 22.8 17.2 21.4 17.2H18.2C16.6 17.2 15.5 16.6 15.5 15Z" fill="#16171A"></path>
<path d="M24 13H30" stroke="#16171A" stroke-width="0.9"></path>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::WeekendSecond => (
            "The pigeon lounging beside a stack of pancakes",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F6C76B 26%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="160" cy="28" r="10" fill="#F6C24C" opacity="0.85"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<ellipse cx="66" cy="99" rx="17" ry="2.6" fill="#FFFFFF" stroke="var(--line-strong, rgba(15,17,20,0.15))"></ellipse>
<ellipse cx="66" cy="96" rx="13" ry="3" fill="#E2A456" stroke="#B8742C" stroke-width="0.7"></ellipse>
<ellipse cx="66" cy="92.5" rx="13" ry="3" fill="#E2A456" stroke="#B8742C" stroke-width="0.7"></ellipse>
<ellipse cx="66" cy="89" rx="13" ry="3" fill="#E9B46A" stroke="#B8742C" stroke-width="0.7"></ellipse>
<rect x="63" y="84.6" width="6" height="3" rx="0.8" fill="#F7E08A"></rect>
<path d="M57 89.5C57 93 58 95 59 96.5" fill="none" stroke="#9A5B1F" stroke-width="1.6" stroke-linecap="round"></path>
<g transform="translate(90 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<path d="M17.6 14.4Q19.5 16 21.4 14.4" fill="none" stroke="#1F3A5C" stroke-width="1.2" stroke-linecap="round"></path>
<path d="M28 45V54M35 45L39 49" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Back => (
            "The pigeon home again with a travel-stickered suitcase",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, var(--accent, #0A7A64) 12%, var(--bg-0, #FFFFFF))"></rect>
<path d="M150 28L176 20L170 34L164 30Z" fill="#FFFFFF" stroke="#A3A6AE" stroke-width="0.8"></path>
<path d="M150 28C138 32 132 40 128 38" fill="none" stroke="#A3A6AE" stroke-width="0.8" stroke-dasharray="2 2"></path>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<path d="M60 80V76H74V80" fill="none" stroke="#7E4A2C" stroke-width="2"></path>
<rect x="48" y="80" width="38" height="20" rx="3" fill="#B5734A"></rect>
<path d="M56 80V100M78 80V100" stroke="#7E4A2C" stroke-width="1.4"></path>
<circle cx="67" cy="90" r="4" fill="var(--accent, #0A7A64)"></circle>
<rect x="70" y="84" width="6" height="4" rx="0.8" fill="#F2B44B"></rect>
<rect x="59" y="91" width="5" height="5" rx="2.5" fill="#8EC5E8"></rect>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::NewYear => (
            "The pigeon in a party hat with a noisemaker and confetti",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, var(--accent, #0A7A64) 12%, var(--bg-0, #FFFFFF))"></rect>
<rect class="greet-fall" x="30" y="12" width="3" height="5" fill="#F08A5D"></rect>
<rect class="greet-fall2" x="52" y="24" width="3" height="5" fill="#8EC5E8"></rect>
<rect class="greet-fall" x="150" y="10" width="3" height="5" fill="#F2B44B"></rect>
<rect class="greet-fall2" x="172" y="30" width="3" height="5" fill="var(--accent, #0A7A64)"></rect>
<rect class="greet-fall" x="128" y="6" width="3" height="5" fill="#B6A4F0"></rect>
<rect class="greet-fall2" x="76" y="8" width="3" height="5" fill="#F5A3B8"></rect>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M8 16.4H0" stroke="#F08A5D" stroke-width="2.2"></path>
<path d="M0 16.4C-5 16.4 -6 10 -2 10C1 10 1 13 -1 13" fill="none" stroke="#F08A5D" stroke-width="2"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<path d="M17.6 14.4Q19.5 16 21.4 14.4" fill="none" stroke="#1F3A5C" stroke-width="1.2" stroke-linecap="round"></path>
<path d="M15 11L22 -5L28 11Z" fill="var(--accent, #0A7A64)"></path>
<path d="M17.5 5L26 6.5M19.5 0L24.2 1" stroke="#FFFFFF" stroke-width="1.2"></path>
<circle cx="22" cy="-5.5" r="2.2" fill="#F2B44B"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::BirdDay => (
            "The pigeon wearing a small crown, sparkles around",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F6C76B 24%, var(--bg-0, #FFFFFF))"></rect>
<path class="greet-twinkle" d="M60 34L61.2 37.8L65 39L61.2 40.2L60 44L58.8 40.2L55 39L58.8 37.8Z" fill="#F2B44B"></path>
<path class="greet-twinkle2" d="M160 30L161 33L164 34L161 35L160 38L159 35L156 34L159 33Z" fill="var(--accent, #0A7A64)"></path>
<path class="greet-twinkle2" d="M80 70L80.8 72.2L83 73L80.8 73.8L80 76L79.2 73.8L77 73L79.2 72.2Z" fill="#F2B44B"></path>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M15 10L16 2L19.5 6L22 0L24.5 6L28 2L29 10Z" fill="#F2B44B" stroke="#C88A1E" stroke-width="0.7"></path>
<circle cx="22" cy="7.4" r="1.1" fill="var(--accent, #0A7A64)"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::LeapDay => (
            "The pigeon mid-leap over the number 29",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #8EC5E8 26%, var(--bg-0, #FFFFFF))"></rect>
<text x="40" y="78" font-size="64" font-weight="800" fill="var(--text-1, #16171A)" opacity="0.07" font-family="Inter, sans-serif">29</text>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<ellipse cx="117" cy="101" rx="16" ry="2.4" fill="rgba(0,0,0,0.08)"></ellipse>
<path d="M66 98C72 80 90 76 100 84" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1" stroke-dasharray="2 3"></path>
<g transform="translate(86 32)"><g class="greet-hop">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 24C38 20 48 18 58 20C50 28 40 32 30 32Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45L25 52M35 45L39 51" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::PiDay => (
            "The pigeon beside a pie marked with pi",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, var(--accent, #0A7A64) 12%, var(--bg-0, #FFFFFF))"></rect>
<text x="24" y="44" font-size="24" font-weight="800" fill="var(--text-1, #16171A)" opacity="0.07" font-family="Inter, sans-serif">3.14159</text>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<ellipse cx="68" cy="98" rx="18" ry="3.5" fill="#D9DEE6"></ellipse>
<path d="M51 96C51 87 85 87 85 96Z" fill="#E2A456" stroke="#B8742C" stroke-width="0.8"></path>
<path d="M56 91.5L60 95M64 89.5L70 95M73 89.6L78 94.5" stroke="#B8742C" stroke-width="0.7" fill="none"></path>
<text x="68" y="95" text-anchor="middle" font-size="9" font-weight="800" fill="#7E4A2C" font-family="Inter, sans-serif">π</text>
<path class="greet-steam" d="M64 84C62 80 66 78 64 74" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1.2" stroke-linecap="round"></path>
<path class="greet-steam2" d="M72 84C70 80 74 78 72 74" fill="none" stroke="var(--text-3, #686B73)" stroke-width="1.2" stroke-linecap="round"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Spring => (
            "The pigeon among spring flowers with a butterfly",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #8EC5E8 22%, var(--bg-0, #FFFFFF))"></rect>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, #6DBE7B 32%, var(--bg-0, #FFFFFF))"></path>
<path d="M40 100V86M56 100V90M176 100V88" stroke="#4E9A5C" stroke-width="1.4"></path>
<path d="M40 94C44 92 46 93 47 95C44 96 42 96 40 94Z" fill="#4E9A5C"></path>
<circle cx="40" cy="81" r="3" fill="#F5A3B8"></circle><circle cx="45" cy="84.5" r="3" fill="#F5A3B8"></circle><circle cx="43" cy="90" r="3" fill="#F5A3B8"></circle><circle cx="37" cy="90" r="3" fill="#F5A3B8"></circle><circle cx="35" cy="84.5" r="3" fill="#F5A3B8"></circle>
<circle cx="40" cy="86" r="2.2" fill="#F6C24C"></circle>
<circle cx="56" cy="86" r="2.6" fill="#B6A4F0"></circle><circle cx="60" cy="89" r="2.6" fill="#B6A4F0"></circle><circle cx="56" cy="92" r="2.6" fill="#B6A4F0"></circle><circle cx="52" cy="89" r="2.6" fill="#B6A4F0"></circle>
<circle cx="56" cy="89" r="1.8" fill="#F6C24C"></circle>
<circle cx="176" cy="86" r="3.4" fill="#F6C24C"></circle>
<g class="greet-sway">
<ellipse cx="66" cy="40" rx="4" ry="5.5" fill="#F2B44B"></ellipse>
<ellipse cx="74" cy="40" rx="4" ry="5.5" fill="#F08A5D"></ellipse>
<path d="M70 35V46" stroke="#52555C" stroke-width="1.2" stroke-linecap="round"></path>
</g>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Summer => (
            "The pigeon in sunglasses with an ice cream under a big sun",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F6C76B 30%, var(--bg-0, #FFFFFF))"></rect>
<circle cx="36" cy="28" r="13" fill="#F6C24C"></circle>
<path class="greet-twinkle" d="M36 8V4M36 52V48M16 28H12M60 28H56M22 14L19 11M50 14L53 11M22 42L19 45M50 42L53 45" stroke="#F6C24C" stroke-width="2" stroke-linecap="round"></path>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, #E9C98C 40%, var(--bg-0, #FFFFFF))"></path>
<path d="M62 84L70 100L78 84Z" fill="#E2A456" stroke="#B8742C" stroke-width="0.7"></path>
<path d="M64.5 88L74 88M66.5 92L72.5 92M66 86L72 96M74 86L68 96" stroke="#B8742C" stroke-width="0.5"></path>
<circle cx="66" cy="81" r="5" fill="#F5A3B8"></circle>
<circle cx="74" cy="81" r="5" fill="#A8E0C8"></circle>
<circle cx="70" cy="75" r="5" fill="#FFF1C9"></circle>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<path d="M15.5 12.4H24V15C24 16.6 22.8 17.2 21.4 17.2H18.2C16.6 17.2 15.5 16.6 15.5 15Z" fill="#16171A"></path>
<path d="M24 13H30" stroke="#16171A" stroke-width="0.9"></path>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Autumn => (
            "The pigeon by a pile of falling autumn leaves",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F09A6A 22%, var(--bg-0, #FFFFFF))"></rect>
<g transform="translate(40 18)"><path class="greet-fall" d="M0 0C4 -4 9 -2 10 0C9 2 4 4 0 0Z" fill="#E07B39"></path></g>
<g transform="translate(150 12)"><path class="greet-fall2" d="M0 0C4 -4 9 -2 10 0C9 2 4 4 0 0Z" fill="#C9472F"></path></g>
<g transform="translate(64 38)"><path class="greet-fall2" d="M0 0C4 -4 9 -2 10 0C9 2 4 4 0 0Z" fill="#E8A93C"></path></g>
<g transform="translate(172 44)"><path class="greet-fall" d="M0 0C4 -4 9 -2 10 0C9 2 4 4 0 0Z" fill="#E07B39"></path></g>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<path d="M48 100C52 92 60 90 66 91C72 89 80 92 84 100Z" fill="#E07B39"></path>
<path d="M54 96C56 94 60 94 62 96C60 98 56 98 54 96ZM66 94C68 92 72 92 74 94C72 96 68 96 66 94ZM74 98C76 96 80 96 82 98C80 100 76 100 74 98Z" fill="#C9472F"></path>
<path d="M60 92C62 90 66 90 68 92C66 94 62 94 60 92Z" fill="#E8A93C"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Winter => (
            "The pigeon in earmuffs as snow falls",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #9DB8D9 32%, var(--bg-0, #FFFFFF))"></rect>
<circle class="greet-fall" cx="30" cy="14" r="1.8" fill="#FFFFFF"></circle>
<circle class="greet-fall2" cx="58" cy="24" r="1.5" fill="#FFFFFF"></circle>
<circle class="greet-fall" cx="150" cy="10" r="1.8" fill="#FFFFFF"></circle>
<circle class="greet-fall2" cx="176" cy="34" r="1.5" fill="#FFFFFF"></circle>
<circle class="greet-fall" cx="120" cy="20" r="1.3" fill="#FFFFFF"></circle>
<circle class="greet-fall2" cx="84" cy="40" r="1.6" fill="#FFFFFF"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, #FFFFFF 78%, var(--bg-0, #FFFFFF))"></path>
<path d="M0 100C30 96 60 97 90 100" fill="none" stroke="#DCE6F2" stroke-width="2"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M15 21C18 26 23 28 28 26.5" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="4.2" stroke-linecap="round"></path>
<path d="M27 26L29 33" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M16 9C18 4 26 4 28 11" fill="none" stroke="#8A93A6" stroke-width="1.6"></path>
<circle cx="26.5" cy="14" r="3.6" fill="#F5A3B8"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
        Scene::Halloween => (
            "The pigeon dressed as a ghost in a sheet, beside a carved pumpkin",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="#2A1F40"></rect>
<circle cx="160" cy="28" r="14" fill="#F4E7B5"></circle>
<circle class="greet-twinkle" cx="40" cy="22" r="1.1" fill="#FFFFFF"></circle>
<circle class="greet-twinkle2" cx="100" cy="14" r="1" fill="#FFFFFF"></circle>
<circle class="greet-twinkle" cx="186" cy="56" r="1" fill="#FFFFFF"></circle>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="#1A1430"></path>
<rect x="60.5" y="79" width="3" height="5" rx="1" fill="#4E7A3A"></rect>
<ellipse cx="62" cy="91" rx="13" ry="9" fill="#E8822E"></ellipse>
<path d="M56 83C53 88 53 95 56 99.5M68 83C71 88 71 95 68 99.5M62 82V100" fill="none" stroke="#C2611A" stroke-width="0.8"></path>
<path d="M55 89L58 85.5L60.5 89ZM63.5 89L66 85.5L69 89ZM55.5 93Q62 98 68.5 93Q62 95.5 55.5 93Z" fill="#3A2207"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M28 45V54M35 45V54" stroke="var(--warning, #F2B44B)" stroke-width="2.4" stroke-linecap="round"></path>
<path d="M10 18C10 4 30 2 34 16C44 22 56 28 60 34L58 44C50 46 44 47 38 46L34 49L30 46L26 49L22 46L18 49L14 46C11 40 10 30 10 18Z" fill="#F4F4F2" stroke="rgba(255,255,255,0.3)" stroke-width="1"></path>
<ellipse cx="17" cy="15" rx="1.5" ry="2.2" fill="#1F2433"></ellipse>
<ellipse cx="23" cy="15" rx="1.5" ry="2.2" fill="#1F2433"></ellipse>
</g></g>"###,
        ),
        Scene::Anniversary => (
            "The pigeon in a party hat beside a cake with one candle",
            r###"<rect class="greet-sky" width="200" height="125" rx="14" fill="color-mix(in srgb, #F6C76B 26%, var(--bg-0, #FFFFFF))"></rect>
<rect class="greet-fall" x="150" y="12" width="3" height="5" fill="#F08A5D"></rect>
<rect class="greet-fall2" x="172" y="30" width="3" height="5" fill="var(--accent, #0A7A64)"></rect>
<rect class="greet-fall" x="34" y="18" width="3" height="5" fill="#8EC5E8"></rect>
<path class="greet-ground" d="M0 100H200V111C200 118.7 193.7 125 186 125H14C6.3 125 0 118.7 0 111Z" fill="color-mix(in srgb, var(--text-1, #16171A) 7%, var(--bg-0, #FFFFFF))"></path>
<rect x="52" y="84" width="32" height="16" rx="2" fill="#F5D7DE" stroke="#D99AAB" stroke-width="0.8"></rect>
<path d="M52 88C55 91 58 91 60 88C62 91 66 91 68 88C70 91 74 91 76 88C78 91 82 91 84 88V86C84 85 83 84 82 84H54C53 84 52 85 52 86Z" fill="#FFFFFF"></path>
<rect x="66.5" y="74" width="3" height="10" rx="0.6" fill="var(--accent, #0A7A64)"></rect>
<path class="greet-twinkle" d="M68 67C70 70 70.5 72 68 74C65.5 72 66 70 68 67Z" fill="#F6B94C"></path>
<g transform="translate(86 46)"><g class="greet-bird">
<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong, rgba(15,17,20,0.15))" stroke-width="1"></path>
<path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"></path>
<path d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent, #0A7A64)" stroke-width="3" stroke-linecap="round"></path>
<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning, #9A5B00)"></path>
<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"></circle>
<path d="M15 11L22 -5L28 11Z" fill="#F08A5D"></path>
<path d="M17.5 5L26 6.5M19.5 0L24.2 1" stroke="#FFFFFF" stroke-width="1.2"></path>
<circle cx="22" cy="-5.5" r="2.2" fill="#F2B44B"></circle>
<path d="M28 45V54M35 45V54" stroke="var(--warning, #9A5B00)" stroke-width="2.4" stroke-linecap="round"></path>
</g></g>"###,
        ),
    }
}
