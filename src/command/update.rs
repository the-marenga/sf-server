use sf_api::{
    command::AttributeType, gamestate::items::EquipmentSlot, misc::to_sf_string,
};
use sqlx::Sqlite;
use strum::IntoEnumIterator;

use super::{
    ResponseBuilder, ServerError, ServerResponse, effective_mount,
    get_debug_value_default, in_seconds, item::add_debug_item, now,
    xp_for_next_level,
};
use crate::{SERVER_VERSION, request::Session};

pub(crate) async fn poll(
    session: Session,
    tracking: &str,
    db: &sqlx::Pool<Sqlite>,
    mut builder: ResponseBuilder,
) -> Result<ServerResponse, ServerError> {
    let resp = builder
        .add_key("serverversion")
        .add_val(SERVER_VERSION)
        .add_key("preregister")
        .add_val(0) // TODO: This has values
        .add_val(0)
        .skip_key();

    let char = sqlx::query!(
        "SELECT
        character.pid, --0
        character.level,
        character.experience,
        character.honor,

        portrait.mouth,
        portrait.Hair,
        portrait.Brows,
        portrait.Eyes,
        portrait.Beards,
        portrait.Nose, --10
        portrait.Ears,
        portrait.Extra,
        portrait.Horns,

        character.race,
        character.gender,
        character.class,

        activity.typ as activitytyp,
        activity.sub_type as activitysubtyp,
        activity.busy_until,

        q1.Flavour1 as q1f1, -- 20
        q3.Flavour1 as q3f1,
        q2.Flavour1 as q2f1,

        q1.Flavour2 as q1f2,
        q2.Flavour2 as q2f2,
        q3.Flavour2 as q3f2,

        q1.Monster as q1monster,
        q2.Monster as q2monster,
        q3.Monster as q3monster,

        q1.Location as q1location,
        q2.Location as q2location, -- 30
        q3.Location as q3location,

        character.mount_end,
        character.mount,

        q1.length as q1length,
        q2.length as q2length,
        q3.length as q3length,

        q1.XP as q1xp,
        q3.XP as q3xp,
        q2.XP as q2xp,

        q1.Silver as q1silver, --40
        q2.SILVER as q2silver,
        q3.SILVER as q3silver,

        tavern.tfa,
        tavern.Beer_Drunk,

        Tutorial_Status,

        tavern.Dice_Game_Next_Free,
        tavern.Dice_Games_Remaining,

        character.mushrooms,
        character.silver,
        tavern.QuickSand, -- 50

        description,
        character.name,

        portrait.influencer,

        (
        SELECT count(*)
        FROM CHARACTER AS x
        WHERE x.world_id = character.world_id
          AND (x.honor > character.honor
               OR (x.honor = character.honor
                   AND x.pid <= character.pid))
        )  as `rank!: i64`,
        (
        SELECT count(*)
        FROM CHARACTER AS x
        WHERE x.world_id = character.world_id
        )  as `maxrank!: i64`

        FROM CHARACTER
         NATURAL JOIN activity
         NATURAL JOIN tavern
         NATURAL JOIN portrait
         JOIN quest as q1 on tavern.quest1 = q1.id
         JOIN quest as q2 on tavern.quest2 = q2.id
         JOIN quest as q3 on tavern.quest2 = q3.id
         WHERE character.pid = $1",
        session.player_id
    )
    .fetch_one(db)
    .await?;

    let calendar_info = "12/1/8/1/3/1/25/1/5/1/2/1/3/2/1/1/24/1/18/5/6/1/22/1/\
                         7/1/6/2/8/2/22/2/5/2/2/2/3/3/21/1";

    resp.add_key("messagelist.r");
    resp.add_str(";");

    resp.add_key("combatloglist.s");
    resp.add_str(";");

    resp.add_key("friendlist.r");
    resp.add_str(";");

    resp.add_key("login count");
    resp.add_val(session.login_count);

    resp.skip_key();

    resp.add_key("sessionid");
    resp.add_str(&session.session_id);

    resp.add_key("languagecodelist");
    resp.add_str(
        "ru,20;fi,8;ar,1;tr,23;nl,16;  \
         ,0;ja,14;it,13;sk,21;fr,9;ko,15;pl,17;cs,2;el,5;da,3;en,6;hr,10;de,4;\
         zh,24;sv,22;hu,11;pt,12;es,7;pt-br,18;ro,19;",
    );

    resp.add_key("languagecodelist.r");

    resp.add_key("maxpetlevel");
    resp.add_val(100);

    resp.add_key("calenderinfo");
    resp.add_val(calendar_info);

    resp.skip_key();

    resp.add_key("tavernspecial");
    resp.add_val(0); // 100 if event active
    resp.add_key("tavernspecialsub");
    resp.add_val(0); // 1 << Event

    resp.add_key("tavernspecialend");
    resp.add_val(in_seconds(600));

    resp.add_key("attbonus1(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus2(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus3(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus4(3)");
    resp.add_str("0/0/0/0");
    resp.add_key("attbonus5(3)");
    resp.add_str("0/0/0/0");

    resp.add_key("stoneperhournextlevel");
    resp.add_val(50);

    resp.add_key("woodperhournextlevel");
    resp.add_val(150);

    resp.add_key("fortresswalllevel");
    resp.add_val(5);

    resp.add_key("inboxcapacity");
    resp.add_val(100);

    resp.add_key("ownplayersave.playerSave");
    resp.add_val(403127023); // What is this?
    resp.add_val(session.player_id);
    resp.add_val(0);
    resp.add_val(1708336503);
    resp.add_val(1292388336);
    resp.add_val(0);
    resp.add_val(0);
    let level = char.level;
    resp.add_val(level); // Level | Arena << 16
    resp.add_val(char.experience); // Experience
    resp.add_val(xp_for_next_level(level)); // Next Level XP
    let honor = char.honor;
    resp.add_val(honor); // Honor
    let rank = char.rank;
    resp.add_val(rank); // Rank

    resp.add_val(0); // 12?
    resp.add_val(10); // 13?
    resp.add_val(0); // 14?
    resp.add_val(char.mushrooms); // Mushroms gained
    resp.add_val(0); // 16?

    // Portrait start
    resp.add_val(char.mouth); // mouth
    resp.add_val(char.hair); // hair
    resp.add_val(char.brows); // brows
    resp.add_val(char.eyes); // eyes
    resp.add_val(char.beards); // beards
    resp.add_val(char.nose); // nose
    resp.add_val(char.ears); // ears
    resp.add_val(char.extra); // extra
    resp.add_val(char.horns); // horns
    resp.add_val(char.influencer); // influencer
    resp.add_val(char.race); // race
    resp.add_val(char.gender); // Gender & Mirror
    resp.add_val(char.class); // class

    // Attributes
    for _ in AttributeType::iter() {
        resp.add_val(100); // 30..=34
    }

    // attribute_additions (aggregate from equipment)
    for _ in AttributeType::iter() {
        resp.add_val(0); // 35..=38
    }

    // attribute_times_bought
    for _ in AttributeType::iter() {
        resp.add_val(0); // 40..=44
    }

    resp.add_val(char.activitytyp); // Current action
    resp.add_val(char.activitysubtyp); // Secondary (time busy)
    resp.add_val(char.busy_until); // Busy until

    // Equipment
    for slot in [
        EquipmentSlot::Hat,
        EquipmentSlot::BreastPlate,
        EquipmentSlot::Gloves,
        EquipmentSlot::FootWear,
        EquipmentSlot::Amulet,
        EquipmentSlot::Belt,
        EquipmentSlot::Ring,
        EquipmentSlot::Talisman,
        EquipmentSlot::Weapon,
        EquipmentSlot::Shield,
    ] {
        add_debug_item(resp, format!("{slot:?}").to_lowercase());
    }
    add_debug_item(resp, "inventory1");
    add_debug_item(resp, "inventory2");
    add_debug_item(resp, "inventory3");
    add_debug_item(resp, "inventory4");
    add_debug_item(resp, "inventory5");

    resp.add_val(in_seconds(60 * 60)); // 228

    // Ok, so Flavour 1, Flavour 2 & Monster ID decide =>
    // - The Line they say
    // - the quest name
    // - the quest giver
    resp.add_val(char.q1f1); // 229 Quest1 Flavour1
    resp.add_val(char.q2f1); // 230 Quest2 Flavour1
    resp.add_val(char.q3f1); // 231 Quest3 Flavour1

    resp.add_val(char.q1f2); // 233 Quest2 Flavour2
    resp.add_val(char.q2f2); // 232 Quest1 Flavour2
    resp.add_val(char.q3f2); // 234 Quest3 Flavour2

    resp.add_val(-char.q1monster); // 235 quest 1 monster
    resp.add_val(-char.q2monster); // 236 quest 2 monster
    resp.add_val(-char.q3monster); // 237 quest 3 monster

    resp.add_val(char.q1location); // 238 quest 1 location
    resp.add_val(char.q2location); // 239 quest 2 location
    resp.add_val(char.q3location); // 240 quest 3 location

    let mut mount_end = char.mount_end;
    let mut mount = char.mount;

    let mount_effect = effective_mount(&mut mount_end, &mut mount);

    resp.add_val((char.q1length as f32 * mount_effect) as i32); // 241 quest 1 length
    resp.add_val((char.q2length as f32 * mount_effect) as i32); // 242 quest 2 length
    resp.add_val((char.q3length as f32 * mount_effect) as i32); // 243 quest 3 length

    // Quest 1..=3 items
    for _ in 0..3 {
        for _ in 0..12 {
            resp.add_val(0); // 244..=279
        }
    }

    resp.add_val(char.q1xp); // 280 quest 1 xp
    resp.add_val(char.q2xp); // 281 quest 2 xp
    resp.add_val(char.q3xp); // 282 quest 3 xp

    resp.add_val(char.q1silver); // 283 quest 1 silver
    resp.add_val(char.q2silver); // 284 quest 2 silver
    resp.add_val(char.q3silver); // 285 quest 3 silver

    resp.add_val(mount); // Mount?

    // Weapon shop
    resp.add_val(1708336503); // 287
    for _ in 0..6 {
        add_debug_item(resp, "weapon");
    }

    // Magic shop
    resp.add_val(1708336503); // 360
    for _ in 0..6 {
        add_debug_item(resp, "weapon");
    }

    resp.add_val(0); // 433
    resp.add_val(1); // 434 might be tutorial related?
    resp.add_val(0); // 435
    resp.add_val(0); // 436
    resp.add_val(0); // 437

    resp.add_val(0); // 438 scrapbook count
    resp.add_val(0); // 439
    resp.add_val(0); // 440
    resp.add_val(0); // 441
    resp.add_val(0); // 442

    resp.add_val(0); // 443 guild join date
    resp.add_val(0); // 444
    resp.add_val(0); // 445 character_hp_bonus << 24, damage_bonus << 16
    resp.add_val(0); // 446
    resp.add_val(0); // 447  Armor
    resp.add_val(6); // 448  Min damage
    resp.add_val(12); // 449 Max damage
    resp.add_val(112); // 450
    resp.add_val(mount_end); // 451 Mount end
    resp.add_val(0); // 452
    resp.add_val(0); // 453
    resp.add_val(0); // 454
    resp.add_val(1708336503); // 455
    resp.add_val(char.tfa); // 456 Alu secs
    resp.add_val(char.beer_drunk); // 457 Beer drunk
    resp.add_val(0); // 458
    resp.add_val(0); // 459 dungeon_timer
    resp.add_val(1708336503); // 460 Next free fight
    resp.add_val(0); // 461
    resp.add_val(0); // 462
    resp.add_val(0); // 463
    resp.add_val(0); // 464
    resp.add_val(408); // 465
    resp.add_val(0); // 466
    resp.add_val(0); // 467
    resp.add_val(0); // 468
    resp.add_val(0); // 469
    resp.add_val(0); // 470
    resp.add_val(0); // 471
    resp.add_val(0); // 472
    resp.add_val(0); // 473
    resp.add_val(-111); // 474
    resp.add_val(0); // 475
    resp.add_val(0); // 476
    resp.add_val(4); // 477
    resp.add_val(1708336504); // 478
    resp.add_val(0); // 479
    resp.add_val(0); // 480
    resp.add_val(0); // 481
    resp.add_val(0); // 482
    resp.add_val(0); // 483
    resp.add_val(0); // 484
    resp.add_val(0); // 485
    resp.add_val(0); // 486
    resp.add_val(0); // 487
    resp.add_val(0); // 488
    resp.add_val(0); // 489
    resp.add_val(0); // 490

    resp.add_val(0); // 491 aura_level (0 == locked)
    resp.add_val(0); // 492 aura_now

    // Active potions
    for _ in 0..3 {
        resp.add_val(0); // typ & size
    }
    for _ in 0..3 {
        resp.add_val(0); // ??
    }
    for _ in 0..3 {
        resp.add_val(0); // expires
    }
    resp.add_val(0); // 502
    resp.add_val(0); // 503
    resp.add_val(0); // 504
    resp.add_val(0); // 505
    resp.add_val(0); // 506
    resp.add_val(0); // 507
    resp.add_val(0); // 508
    resp.add_val(0); // 509
    resp.add_val(0); // 510
    resp.add_val(6); // 511
    resp.add_val(2); // 512
    resp.add_val(0); // 513
    resp.add_val(0); // 514
    resp.add_val(100); // 515 aura_missing
    resp.add_val(0); // 516
    resp.add_val(0); // 517
    resp.add_val(0); // 518
    resp.add_val(100); // 519
    resp.add_val(0); // 520
    resp.add_val(0); // 521
    resp.add_val(0); // 522
    resp.add_val(0); // 523

    // Fortress
    // Building levels
    resp.add_val(0); // 524
    resp.add_val(0); // 525
    resp.add_val(0); // 526
    resp.add_val(0); // 527
    resp.add_val(0); // 528
    resp.add_val(0); // 529
    resp.add_val(0); // 530
    resp.add_val(0); // 531
    resp.add_val(0); // 532
    resp.add_val(0); // 533
    resp.add_val(0); // 534
    resp.add_val(0); // 535
    resp.add_val(0); // 536
    resp.add_val(0); // 537
    resp.add_val(0); // 538
    resp.add_val(0); // 539
    resp.add_val(0); // 540
    resp.add_val(0); // 541
    resp.add_val(0); // 542
    resp.add_val(0); // 543
    resp.add_val(0); // 544
    resp.add_val(0); // 545
    resp.add_val(0); // 546
    // unit counts
    resp.add_val(0); // 547
    resp.add_val(0); // 548
    resp.add_val(0); // 549
    // upgrade_began
    resp.add_val(0); // 550
    resp.add_val(0); // 551
    resp.add_val(0); // 552
    // upgrade_finish
    resp.add_val(0); // 553
    resp.add_val(0); // 554
    resp.add_val(0); // 555

    resp.add_val(0); // 556
    resp.add_val(0); // 557
    resp.add_val(0); // 558
    resp.add_val(0); // 559
    resp.add_val(0); // 560
    resp.add_val(0); // 561

    // Current resource in store
    resp.add_val(0); // 562
    resp.add_val(0); // 563
    resp.add_val(0); // 564
    // max_in_building
    resp.add_val(0); // 565
    resp.add_val(0); // 566
    resp.add_val(0); // 567
    // max saved
    resp.add_val(0); // 568
    resp.add_val(0); // 569
    resp.add_val(0); // 570

    resp.add_val(0); // 571 building_upgraded
    resp.add_val(0); // 572 building_upgrade_finish
    resp.add_val(0); // 573 building_upgrade_began
    // per hour
    resp.add_val(0); // 574
    resp.add_val(0); // 575
    resp.add_val(0); // 576
    resp.add_val(0); // 577 unknown time stamp
    resp.add_val(0); // 578

    resp.add_val(0); // 579 wheel_spins_today
    resp.add_val(now() + 60 * 10); // 580  wheel_next_free_spin

    resp.add_val(0); // 581 ft level
    resp.add_val(100); // 582 ft honor
    resp.add_val(0); // 583 rank
    resp.add_val(900); // 584
    resp.add_val(300); // 585
    resp.add_val(0); // 586

    resp.add_val(0); // 587 attack target
    resp.add_val(0); // 588 attack_free_reroll
    resp.add_val(0); // 589
    resp.add_val(0); // 590
    resp.add_val(0); // 591
    resp.add_val(0); // 592
    resp.add_val(3); // 593

    resp.add_val(0); // 594 gem_stone_target
    resp.add_val(0); // 595 gem_search_finish
    resp.add_val(0); // 596 gem_search_began
    resp.add_val(char.tutorial_status); // 597 Pretty sure this is a bit map of which messages have been seen
    resp.add_val(0); // 598

    // Arena enemies
    resp.add_val(get_debug_value_default("arena_enemy1", 1)); // 599
    resp.add_val(get_debug_value_default("arena_enemy2", 1)); // 600
    resp.add_val(get_debug_value_default("arena_enemy3", 1)); // 601

    resp.add_val(0); // 602
    resp.add_val(0); // 603
    resp.add_val(0); // 604
    resp.add_val(0); // 605
    resp.add_val(0); // 606
    resp.add_val(0); // 607
    resp.add_val(0); // 608
    resp.add_val(0); // 609
    resp.add_val(1708336504); // 610
    resp.add_val(0); // 611
    resp.add_val(0); // 612
    resp.add_val(0); // 613
    resp.add_val(0); // 614
    resp.add_val(0); // 615
    resp.add_val(0); // 616
    resp.add_val(1); // 617
    resp.add_val(0); // 618
    resp.add_val(0); // 619
    resp.add_val(0); // 620
    resp.add_val(0); // 621
    resp.add_val(0); // 622
    resp.add_val(0); // 623 own_treasure_skill
    resp.add_val(0); // 624 own_instr_skill
    resp.add_val(0); // 625
    resp.add_val(30); // 626
    resp.add_val(0); // 627 hydra_next_battle
    resp.add_val(0); // 628 remaining_pet_battles
    resp.add_val(0); // 629
    resp.add_val(0); // 630
    resp.add_val(0); // 631
    resp.add_val(0); // 632
    resp.add_val(0); // 633
    resp.add_val(0); // 634
    resp.add_val(0); // 635
    resp.add_val(0); // 636
    resp.add_val(0); // 637
    resp.add_val(0); // 638
    resp.add_val(0); // 639
    resp.add_val(0); // 640
    resp.add_val(0); // 641
    resp.add_val(0); // 642
    resp.add_val(0); // 643
    resp.add_val(0); // 644
    resp.add_val(0); // 645
    resp.add_val(0); // 646
    resp.add_val(0); // 647
    resp.add_val(0); // 648
    resp.add_val(in_seconds(60 * 60)); // 649 calendar_next_possible
    resp.add_val(char.dice_game_next_free); // 650 dice_games_next_free
    resp.add_val(char.dice_games_remaining); // 651 dice_games_remaining
    resp.add_val(0); // 652
    resp.add_val(0); // 653 druid mask
    resp.add_val(0); // 654
    resp.add_val(0); // 655
    resp.add_val(0); // 656
    resp.add_val(6); // 657
    resp.add_val(0); // 658
    resp.add_val(2); // 659
    resp.add_val(0); // 660 pet dungeon timer
    resp.add_val(0); // 661
    resp.add_val(0); // 662
    resp.add_val(0); // 663
    resp.add_val(0); // 664
    resp.add_val(0); // 665
    resp.add_val(0); // 666
    resp.add_val(0); // 667
    resp.add_val(0); // 668
    resp.add_val(0); // 669
    resp.add_val(0); // 670
    resp.add_val(1950020000000i64); // 671
    resp.add_val(0); // 672
    resp.add_val(0); // 673
    resp.add_val(0); // 674
    resp.add_val(0); // 675
    resp.add_val(0); // 676
    resp.add_val(0); // 677
    resp.add_val(0); // 678
    resp.add_val(0); // 679
    resp.add_val(0); // 680
    resp.add_val(0); // 681
    resp.add_val(0); // 682
    resp.add_val(0); // 683
    resp.add_val(0); // 684
    resp.add_val(0); // 685
    resp.add_val(0); // 686
    resp.add_val(0); // 687
    resp.add_val(0); // 688
    resp.add_val(0); // 689
    resp.add_val(0); // 690
    resp.add_val(0); // 691
    resp.add_val(1); // 692
    resp.add_val(0); // 693
    resp.add_val(0); // 694
    resp.add_val(0); // 695
    resp.add_val(0); // 696
    resp.add_val(0); // 697
    resp.add_val(0); // 698
    resp.add_val(0); // 699
    resp.add_val(0); // 700
    resp.add_val(0); // 701 bard instrument
    resp.add_val(0); // 702
    resp.add_val(0); // 703
    resp.add_val(1); // 704
    resp.add_val(0); // 705
    resp.add_val(0); // 706
    resp.add_val(0); // 707
    resp.add_val(0); // 708
    resp.add_val(0); // 709
    resp.add_val(0); // 710
    resp.add_val(0); // 711
    resp.add_val(0); // 712
    resp.add_val(0); // 713
    resp.add_val(0); // 714
    resp.add_val(0); // 715
    resp.add_val(0); // 716
    resp.add_val(0); // 717
    resp.add_val(0); // 718
    resp.add_val(0); // 719
    resp.add_val(0); // 720
    resp.add_val(0); // 721
    resp.add_val(0); // 722
    resp.add_val(0); // 723
    resp.add_val(0); // 724
    resp.add_val(0); // 725
    resp.add_val(0); // 726
    resp.add_val(0); // 727
    resp.add_val(0); // 728
    resp.add_val(0); // 729
    resp.add_val(0); // 730
    resp.add_val(0); // 731
    resp.add_val(0); // 732
    resp.add_val(0); // 733
    resp.add_val(0); // 734
    resp.add_val(0); // 735
    resp.add_val(0); // 736
    resp.add_val(0); // 737
    resp.add_val(0); // 738
    resp.add_val(0); // 739
    resp.add_val(0); // 740
    resp.add_val(0); // 741
    resp.add_val(0); // 742
    resp.add_val(0); // 743
    resp.add_val(0); // 744
    resp.add_val(0); // 745
    resp.add_val(0); // 746
    resp.add_val(0); // 747
    resp.add_val(0); // 748
    resp.add_val(0); // 749
    resp.add_val(0); // 750
    resp.add_val(0); // 751
    resp.add_val(0); // 752
    resp.add_val(0); // 753
    resp.add_val(0); // 754
    resp.add_val(0); // 755
    resp.add_val(0); // 756
    resp.add_val(0); // 757
    resp.add_str(""); // 758

    resp.add_key("resources");
    resp.add_val(session.player_id); // pid
    resp.add_val(char.mushrooms); // mushrooms
    resp.add_val(char.silver); // silver
    resp.add_val(0); // lucky coins
    resp.add_val(char.quicksand); // quicksand glasses
    resp.add_val(0); // wood
    resp.add_val(0); // ??
    resp.add_val(0); // stone
    resp.add_val(0); // ??
    resp.add_val(0); // metal
    resp.add_val(0); // arcane
    resp.add_val(0); // souls
    // Fruits
    for _ in 0..5 {
        resp.add_val(0);
    }

    resp.add_key("owndescription.s");
    resp.add_str(&to_sf_string(&char.description));

    resp.add_key("ownplayername.r");
    resp.add_str(&char.name);

    let maxrank = char.maxrank;

    resp.add_key("maxrank");
    resp.add_val(maxrank);

    resp.add_key("skipallow");
    resp.add_val(0);

    resp.add_key("skipvideo");
    resp.add_val(0);

    resp.add_key("fortresspricereroll");
    resp.add_val(18);

    resp.add_key("timestamp");

    resp.add_val(now());

    resp.add_key("fortressprice.fortressPrice(13)");
    resp.add_str(
        "900/1000/0/0/900/500/35/12/900/200/0/0/900/300/22/0/900/1500/50/17/\
         900/700/7/9/900/500/41/7/900/400/20/14/900/600/61/20/900/2500/40/13/\
         900/400/25/8/900/15000/30/13/0/0/0/0",
    );

    resp.skip_key();

    resp.add_key("unitprice.fortressPrice(3)");
    resp.add_str("600/0/15/5/600/0/11/6/300/0/19/3/");

    resp.add_key("upgradeprice.upgradePrice(3)");
    resp.add_val("28/270/210/28/720/60/28/360/180/");

    resp.add_key("unitlevel(4)");
    resp.add_val("5/25/25/25/");

    resp.skip_key();
    resp.skip_key();

    resp.add_key("petsdefensetype");
    resp.add_val(3);

    resp.add_key("singleportalenemylevel");
    resp.add_val(0);

    resp.skip_key();

    resp.add_key("wagesperhour");
    resp.add_val(10);

    resp.skip_key();

    resp.add_key("dragongoldbonus");
    resp.add_val(30);

    resp.add_key("toilettfull");
    resp.add_val(0);

    resp.add_key("maxupgradelevel");
    resp.add_val(20);

    resp.add_key("cidstring");
    resp.add_str("no_cid");

    if !tracking.is_empty() {
        resp.add_key("tracking.s");
        resp.add_str(tracking);
    }

    resp.add_key("calenderinfo");
    resp.add_str(calendar_info);

    resp.skip_key();

    resp.add_key("iadungeontime");
    resp.add_str("5/1702656000/1703620800/1703707200");

    resp.add_key("achievement(208)");
    resp.add_str(
        "0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/0/\
         0/0/0/0/",
    );

    resp.add_key("scrapbook.r");
    resp.add_str("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==");

    resp.add_key("smith");
    resp.add_str("5/0");

    resp.add_key("owntowerlevel");
    resp.add_val(0);

    resp.add_key("webshopid");
    resp.add_str("Q7tGCJhe$r464");

    resp.add_key("dailytasklist");
    resp.add_val(98);
    for typ_id in 1..=10 {
        resp.add_val(typ_id); // typ
        resp.add_val(0); // current
        resp.add_val(typ_id); // target
        resp.add_val(10); // reward
    }

    resp.add_key("eventtasklist");
    for typ_id in 1..=99 {
        if typ_id == 73 {
            continue;
        }
        resp.add_val(typ_id); // typ
        resp.add_val(0); // current
        resp.add_val(typ_id); // target
        resp.add_val(typ_id); // reward
    }

    resp.add_key("dailytaskrewardpreview");
    add_reward_previews(resp);

    resp.add_key("eventtaskrewardpreview");

    add_reward_previews(resp);

    resp.add_key("eventtaskinfo");
    resp.add_val(1708300800);
    resp.add_val(1798646399);
    resp.add_val(2); // event typ

    resp.add_key("unlockfeature");

    let dungeon_progress_light = [
        0,  // DesecratedCatacombs = 0,
        0,  // MinesOfGloria = 1,
        0,  // RuinsOfGnark = 2,
        0,  // CutthroatGrotto = 3,
        0,  // EmeraldScaleAltar = 4,
        0,  // ToxicTree = 5,
        0,  // MagmaStream = 6,
        0,  // FrostBloodTemple = 7,
        0,  // PyramidsofMadness = 8,
        0,  // BlackSkullFortress = 9,
        0,  // CircusOfHorror = 10,
        0,  // Hell = 11,
        0,  // The13thFloor = 12,
        6,  // Easteros = 13,
        86, // Tower = 14,
        1,  // TimeHonoredSchoolofMagic = 15,
        2,  // Hemorridor = 16,
        27, // Portal = 17
        1,  // NordicGods = 18,
        1,  // MountOlympus = 19,
        0,  // TavernoftheDarkDoppelgangers = 20,
        3,  // DragonsHoard = 21,
        0,  // HouseOfHorrors = 22,
        0,  // ThirdLeagueOfSuperheroes = 23,
        1,  // DojoOfChildhoodHeroes = 24,
        1,  // MonsterGrotto = 25,
        1,  // CityOfIntrigues = 26,
        1,  // SchoolOfMagicExpress = 27,
        1,  // AshMountain = 28,
        0,  // PlayaGamesHQ = 29,
        10, // TrainingCamp = 30,
        1,  // Sandstorm = 31,
        1,  // Old Pixel = 32
        2,  // Server Room = 33
        3,  // Workshop = 34
        4,  // Retro TV = 35
        5,  // Meeting room = 36
    ];

    resp.add_key(&format!(
        "dungeonprogresslight({})",
        dungeon_progress_light.len()
    ));
    for val in dungeon_progress_light {
        resp.add_val(val);
    }

    let dungeon_progress_shadow = [
        10,  // DesecratedCatacombs = 0,
        10,  // MinesOfGloria = 1,
        10,  // RuinsOfGnark = 2,
        10,  // CutthroatGrotto = 3,
        10,  // EmeraldScaleAltar = 4,
        10,  // ToxicTree = 5,
        10,  // MagmaStream = 6,
        10,  // FrostBloodTemple = 7,
        10,  // PyramidsOfMadness = 8,
        4,   // BlackSkullFortress = 9,
        1,   // CircusOfHorror = 10,
        1,   // Hell = 11,
        1,   // The13thFloor = 12,
        1,   // Easteros = 13,
        316, // Twister = 14,
        1,   // TimeHonoredSchoolOfMagic = 15,
        1,   // Hemorridor = 16,
        0,   // ContinuousLoopofIdols = 17,
        0,   // NordicGods = 18,
        0,   // MountOlympus = 19,
        1,   // TavernOfTheDarkDoppelgangers = 20,
        1,   // DragonsHoard = 21,
        1,   // HouseOfHorrors = 22,
        1,   // ThirdLeagueofSuperheroes = 23,
        1,   // DojoOfChildhoodHeroes = 24,
        1,   // MonsterGrotto = 25,
        1,   // CityOfIntrigues = 26,
        1,   // SchoolOfMagicExpress = 27,
        1,   // AshMountain = 28,
        1,   // PlayaGamesHQ = 29,
        0,   // ?
        0,   // ?
        1,   // Old Pixel = 32
        2,   // Server Room = 33
        3,   // Workshop = 34
        4,   // Retro TV = 35
        5,   // Meeting room = 36
    ];

    resp.add_key(&format!(
        "dungeonprogressshadow({})",
        dungeon_progress_shadow.len()
    ));
    for val in dungeon_progress_shadow {
        resp.add_val(val);
    }

    let dungeon_info = calc_dungeon_progress(&dungeon_progress_light, false);

    resp.add_key(&format!("dungeonenemieslight({})", dungeon_info.len()));
    for dungeon in &dungeon_info {
        resp.add_val(dungeon.enemy.monster_name);
        resp.add_val(dungeon.id);
        resp.add_val(dungeon.enemy.loot);
    }

    resp.add_key(&format!(
        "currentdungeonenemieslight({})",
        dungeon_info.iter().filter(|a| a.is_current).count()
    ));
    for dungeon in dungeon_info.iter().filter(|a| a.is_current) {
        resp.add_val(dungeon.enemy.monster_name);
        resp.add_val(dungeon.id);
        resp.add_val(dungeon.enemy.level);
        resp.add_val(dungeon.enemy.class);
        resp.add_val(dungeon.enemy.element);
    }

    let dungeon_info = calc_dungeon_progress(&dungeon_progress_shadow, true);

    resp.add_key(&format!("dungeonenemiesshadow({})", dungeon_info.len()));
    for dungeon in &dungeon_info {
        resp.add_val(dungeon.enemy.monster_name);
        resp.add_val(dungeon.id);
        resp.add_val(dungeon.enemy.loot);
    }

    resp.add_key(&format!(
        "currentdungeonenemiesshadow({})",
        dungeon_info.iter().filter(|a| a.is_current).count()
    ));
    for dungeon in dungeon_info.iter().filter(|a| a.is_current) {
        resp.add_val(dungeon.enemy.monster_name);
        resp.add_val(dungeon.id);
        resp.add_val(dungeon.enemy.level);
        resp.add_val(dungeon.enemy.class);
        resp.add_val(dungeon.enemy.element);
    }

    resp.add_key("portalprogress(3)");
    resp.add_val("27/100/194");

    resp.skip_key();

    let expeditions = sqlx::query!(
        "SELECT target, alu_sec, location_1, location_2
        FROM expedition
        WHERE pid = $1",
        session.player_id
    )
    .fetch_all(db)
    .await?;

    resp.add_key("expeditions");

    for exp in expeditions {
        resp.add_val(exp.target); // typ
        resp.add_val(71); // ??
        resp.add_val(32); // ??
        resp.add_val(91); // ??
        resp.add_val(exp.location_1); // location 1
        resp.add_val(exp.location_2); // location 2
        resp.add_val(exp.alu_sec); // alu
        resp.add_val(0); // 1 => egg, 2 => inc. daily task
    }

    resp.add_key("expeditionevent");
    resp.add_val(in_seconds(-60 * 60));
    resp.add_val(in_seconds(60 * 60));
    resp.add_val(1);
    resp.add_val(in_seconds(60 * 60));

    resp.add_key("usersettings");
    resp.add_str("en");
    resp.add_val(0);
    resp.add_val(0);
    resp.add_val(0);
    resp.add_str("a");
    resp.add_val(0);

    resp.add_key("mailinvoice");
    resp.add_str("a*******@a****.***");

    resp.add_key("cryptoid");
    resp.add_val(session.crypto_id);

    resp.add_key("cryptokey");
    resp.add_val(session.crypto_key);

    // resp.add_key("pendingrewards");
    // for i in 0..10 {
    //     resp.add_val(9999 + i);
    //     resp.add_val(2);
    //     resp.add_val(i);
    //     resp.add_val("Reward Name");
    //     resp.add_val(1717777586);
    //     resp.add_val(1718382386);
    // }

    resp.build()
}

#[derive(Debug, Clone, Copy)]
struct DungeonEnemy {
    monster_name: u16,
    loot: u8,
    level: u16,
    class: u8,
    element: u8,
}

fn lookup_dungeon(dungeon_id: usize, is_shadow: bool) -> Vec<DungeonEnemy> {
    let limit = match dungeon_id {
        14 if !is_shadow => 100,  // Tower
        17 if !is_shadow => 40,   // Portal
        31 if !is_shadow => 1000, // Sandstorm
        _ => 10,
    };

    (0..limit)
        .map(|_floor| DungeonEnemy {
            monster_name: 1150,
            loot: 0,
            level: dungeon_id as u16,
            class: 1,
            element: 0,
        })
        .collect()
}

struct DungeonInfo {
    id: usize,
    enemy: DungeonEnemy,
    is_current: bool,
}

fn calc_dungeon_progress(
    dungeon_progress: &[i32],
    is_shadow: bool,
) -> Vec<DungeonInfo> {
    let mut dungeon_enemies = Vec::new();

    for (idx, &progress) in dungeon_progress.iter().enumerate() {
        let dungeon = lookup_dungeon(idx, is_shadow);
        if progress >= dungeon.len() as i32 || progress < 0 {
            continue;
        }

        for offset in &[-2, -1, 0, 1, 2] {
            let pos = progress + offset;
            if pos < 0 {
                continue;
            }
            if let Some(enemy) = dungeon.get(pos as usize).copied() {
                dungeon_enemies.push(DungeonInfo {
                    id: idx + 1,
                    enemy,
                    is_current: *offset == 0,
                });
            }
        }
    }

    dungeon_enemies
}

fn add_reward_previews(resp: &mut ResponseBuilder) {
    for i in 1..=3 {
        resp.add_val(0);
        resp.add_val(match i {
            1 => 400,
            2 => 123,
            _ => 999,
        });
        let count = 16;
        resp.add_val(count);
        // amount of rewards
        for i in 0..count {
            resp.add_val(i + 1); // typ
            resp.add_val(1000); // typ amount
        }
    }
}
