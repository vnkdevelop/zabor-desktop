const JOKE_TRANSLATIONS: Record<string, string> = {
  "Дал девушке средство для ухода, а она не уходит.": "I gave my girlfriend care products, but she still won't leave.",
  "Учитель харакири: \"Смотри, показываю один раз..\"": "Harakiri teacher: \"Look, I'll only show you this once...\"",
  "Что сказало одна стена другой? \nВстретимся на углу.": "What did one wall say to the other? \nMeet you at the corner.",
  "Если бы моя бабушка знала, сколько денег я сэкономил на ее похоронах, то она бы перевернулась в канаве.": "If my grandmother knew how much money I saved on her funeral, she'd turn over in her ditch.",
  "Однорукий человек заплакал, увидев магазин «секонд-хенд».": "A one-armed man burst into tears when he saw a second-hand store.",
  "Штирлиц стрелял в слепую. Слепая бегала зигзагами и кричала.": "Stierlitz shot blindly. The blind woman ran in zigzags and screamed.",
  "Гроб карлика-оптимиста наполовину полон.": "The coffin of a dwarf optimist is half full.",
  "Одна девочка так сильно боялась прыгать с парашютом, что прыгнула без него.": "One girl was so afraid of skydiving that she jumped without a parachute.",
  "Говорю девушке:\"Даже мыло хозяйственное, а ты нет.\"": "I told my girlfriend: \"Even laundry soap is domestic, but you're not.\"",
  "Как называют чёрного который накосячил? \nНигадяй.": "What do you call a black guy who messed up? \nA scumbag.",
  "Бежит мальчик по лесу, видит реку, переплыл её, потом вспомнил что не умеет плавать, вернулся и утонул.": "A boy runs through the forest, sees a river, swims across it, then remembers he can't swim, returns, and drowns.",
  "Почему грустит дочка в семье антенн? \nОна приёмная.": "Why is the daughter in the antenna family sad? \nShe is adopted.",
  "Как называется избушка Бабы-Яги лесбиянки?\n Лесбушка.": "What is Baba Yaga's lesbian hut called?\n Lesbushka.",
  "Почему безногий боится гопников? Не может постоять за себя.": "Why is a legless man afraid of street thugs? He can't stand up for himself.",
  "Олег Монгол с сыном: - Пап смотри голубь, у тебя хлеб есть? - Без хлеба ешь.": "Oleg Mongol with his son: - Dad, look, a pigeon! Do you have bread? - Eat it without bread.",
  "Почему Ник Вуйчич не играет в твистер? \nОн дальтоник.": "Why doesn't Nick Vujicic play Twister? \nHe's colorblind.",
  "Что орал парень когда его раздавило в шахте лифта? \nЗакладчик гондон!": "What did the guy scream when he got crushed in the elevator shaft? \n\"The courier is a scumbag!\"",
  "У моей подруги умерла собака, поэтому я купил ей точно такую же, она закричала: - Что мне делать с 2 мёртвыми собаками?": "My friend's dog died, so I bought her an exact replica. She screamed: - What am I supposed to do with two dead dogs?",
  "Какую часть овоща есть сложнее всего? Инвалидное кресло.\n": "What's the hardest part of a vegetable to eat? The wheelchair.",
  "Как называют чёрную женщину сделавшую 7 абортов? \nБорец с преступностью.": "What do you call a black woman who had 7 abortions? \nA crime fighter.",
  "Как предотвратить инцест грибов? Фразой не спорь с матерью.": "How do you prevent mushroom incest? By saying: \"don't argue with your mother/spawn.\"",
  "Жареный хлеб, звучит как тост.": "Toasted bread sounds like a toast.",
  "Маленькое, чёрное и в стекло бьётся что это? \nМладенец в духовке.": "Small, black, and hitting the glass—what is it? \nA baby in the oven.",
  "Почему Ник Вуйчич не написал тест? \nОн забыл ручку.": "Why didn't Nick Vujicic write the test? \nHe forgot his pen.",
  "Почему глухонемая девочка удовлетворяет себя одной рукой? \nВторой стонет.": "Why does a deaf-mute girl satisfy herself with one hand? \nShe moans with the other.",
  "На чемпионате мира по вежливости победил питерский бездомный алкаш, которому не хватало 12 рублей.": "At the world courtesy championship, a homeless alcoholic from St. Petersburg won because he was 12 rubles short.",
  "Наиболее яркий пример использования оружия в мирных целях - это забивание гвоздей гранатой.": "The most striking example of using weapons for peaceful purposes is hammering nails with a grenade.",
  "Приходит чукча с сыном к врачу и говорит: Доктор, мой сын ничего не ест: ни масла, ни мяса, ни хлеба. Почему? Нету.": "A Chukchi and his son visit the doctor: \"Doctor, my son doesn't eat anything: no butter, no meat, no bread. Why?\" - \"We don't have any.\"",
  "В 1874 году Жан Посижу изобрёл посидеть когда пытался стоять у стула с согнутыми коленями.": "In 1874, Jean I-Will-Sit invented sitting down when he tried to stand near a chair with bent knees.",
  "Ебутся как-то два клоуна,один говорит: \"Cмотри, как в анекдоте получается\" и рассказывает:\" ебутся как-то два клоуна..\"": "Two clowns are having sex, one says: \"Look, just like in the joke\" and tells: \"Two clowns are having sex...\"",
  "Один парень сказал: Да я на пальцах одной руки могу перечислить 11 аргументов против инцеста.": "One guy said: \"I can list 11 arguments against incest on the fingers of one hand.\"",
  "Два термита заходят в ресторан. Официант: - Что будете заказывать? \nСтолик на двоих.": "Two termites walk into a restaurant. Waiter: - What would you like to order? \n- A table for two.",
  "В Германии прошёл фестиваль пива Больше всех пива выпил житель Тамбова - Олег Иванович, который смотрел фестиваль по телевизору.": "A beer festival took place in Germany. The person who drank the most beer was Oleg Ivanovich from Tambov, who watched the festival on TV.",
  "Маленькую Лизу, глухую на одно ухо, мама ласково называла Моно Лиза.": "Little Lisa, who was deaf in one ear, was affectionately called Mono Lisa by her mother.",
  "Очки были названы в честь выдающегося итальянского ученыго Салавино Очко.": "Glasses (Ochki) were named in honor of the prominent Italian scientist Salavino Ochko.",
  "Однажды показал девушке как я готовлю Карпаччо из березового сока в скорлупе Фаберже, приправленное пылью из под подошвы Илона Маска с тяжёлым вздохом шеф-повара, гарнированная ароматом милфы за 60 и карамелизированным талоном к терапевту, Так она диву прикинулась, пришлось уебать суку.": "Once I showed a girl how I prepare birch sap carpaccio in a Faberge eggshell, seasoned with dust from Elon Musk's shoe sole with a heavy sigh of a chef, garnished with the aroma of a milf over 60 and a caramelized coupon to a therapist. She acted surprised, so I had to hit the bitch.",
  "Идут 100 негров по пустыне... -Почему по пустыне ? \nГород уже сожгли.": "100 black guys are walking across the desert... - Why the desert? \n- The city is already burned down.",
  "Как называют тупой ИИ? \nЫЫ": "What do you call a dumb AI? \nYY",
  "Девушка вышла за механика, и родила шестерню.": "A girl married a mechanic and gave birth to a gear."
};

export const translateJoke = (jokeText: string, language: string): string => {
  if (language === 'en') {
    return JOKE_TRANSLATIONS[jokeText] || jokeText;
  }
  return jokeText;
};
