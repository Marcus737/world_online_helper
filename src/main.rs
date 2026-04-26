use std::{thread};

use anyhow::Result;
use tracing::{error, info};

use crate::funs::{GameHelper, ItemType, Rarity};

mod config_util;
mod funs;
mod mumu_manager;
mod orc_helper;
mod util;

const CHANNLE_MAX_MSG_SIZE: usize = 100;

#[derive(Debug, PartialEq, Clone, Copy)]
enum Msg {
    Exit,
    ClearBag,
    ClickTaskButton,
    AutoFight,
    TurnBacK,
}

const FILTER_NAMES: [&str; 37] = [
    "防具箱（锁）",
    "服饰箱（锁）",
    "防具箱（锁）",
    "治疗药水",
    "法力药水",
    "月狼之石",
    "腐鹰羽毛",
    "灵草",
    "被咬过的肉",
    "断骨",
    "毒舌",
    "腐尸",
    "鹰身人的羽毛",
    "业火",
    "死心",
    "断弩",
    "腐肉",
    "药果",
    "双头犬的肉",
    "灵魂结晶",
    "英雄证明",
    "杜时雨的颜料",
    "竹子",
    "药果",
    "英雄证明",
    "兑换铜币",
    "灵魂结晶",
    "豹皮",
    "三级碎木",
    "三级碎石",
    "三级碎矿",
    "四级碎木",
    "四级碎石",
    "四级碎矿",
    "五级碎木",
    "五级碎石",
    "五级碎矿",
];

#[tokio::main]
async fn main() -> Result<()> {
    util::init_logger();

    let app_config = config_util::AppConfig::load_from_file()?;

    info!("start adb server");
    util::run_command(&app_config.adb_path, vec!["start-server"])?;

    info!("start ocr server");
    let ocr_server = orc_helper::OcrServer::launch()?;

    let (main_send, mut main_recv) = tokio::sync::mpsc::channel::<Msg>(CHANNLE_MAX_MSG_SIZE);
    let mut sender_vec = vec![];


    let (win_width, win_height) = (370, 720);
    let (mut x, y) = (0, 0);
    let client_num = 5;
    for i in 0..client_num {
        let vm_client = mumu_manager::VmClient::new(i, &app_config.manager_path);
        let mut game_helper =
            GameHelper::new(vm_client, ocr_server.get_client(), Some(&app_config.app_package_names[i])).await?;
        game_helper.vm_client.set_layout_window(
            Some(x),
            Some(y),
            Some(win_width),
            Some(win_height),
        )?;
        x += win_width;

        let (send, mut recv) = tokio::sync::mpsc::channel::<Msg>(CHANNLE_MAX_MSG_SIZE);
        sender_vec.push(send);

        tokio::spawn(async move {
            info!("异步任务{}已开启", i);
            loop {
                if let Some(msg) = recv.recv().await {
                    match msg {
                        Msg::Exit => break,
                        Msg::ClearBag => {
                            info!("{} 正在执行清理背包", i);
                            if let Err(e) = game_helper.clear_bag_v2().await
                            {
                                error!("{} ClearBag error:{}", i, e)
                            }
                        }
                        Msg::ClickTaskButton => {
                            info!("{} 点击任务按钮", i);
                            // if let Err(e) = game_helper.click_task_button().await {
                            //     error!("{} ClickTaskButton error:{}", i, e);
                            // }
                            if let Err(e) = game_helper.adb_device.keyevent(12).await {
                                error!("点击任务按钮失败:{}", e);
                            };
                        }
                        Msg::AutoFight => {
                            if let Err(e) = game_helper.adb_device.tap(480, 1200).await {
                                error!("点击自动战斗失败:{}", e);
                            }
                        }
                        Msg::TurnBacK => {
                            if let Err(e) = game_helper.adb_device.keyevent(4).await {
                                error!("点击返回按钮失败:{}", e);
                            };
                        }
                    }
                }
            }
            info!("异步任务{}已结束", i);
        });
    }

    thread::spawn(|| {
        fn send_all(sender_vec: &Vec<tokio::sync::mpsc::Sender<Msg>>, msg: Msg) {
            for sender in sender_vec {
                sender.blocking_send(msg).unwrap();
            }
        }

        rdev::listen(move |evt| {
            if let rdev::EventType::KeyPress(key) = evt.event_type {
                info!("按下:{:?}", key);
                match key {
                    rdev::Key::F12 => {
                        let _ = main_send.blocking_send(Msg::Exit);
                        send_all(&sender_vec, Msg::Exit);
                    }
                    rdev::Key::Kp0 => {
                        send_all(&sender_vec, Msg::ClickTaskButton);
                    }
                    rdev::Key::Kp8 => {
                        send_all(&sender_vec, Msg::ClearBag);
                    }
                    rdev::Key::Kp1 => {
                        send_all(&sender_vec, Msg::AutoFight);
                    }
                    rdev::Key::ControlRight => {
                        send_all(&sender_vec, Msg::TurnBacK);
                    }
                    _ => {}
                }
            }
        })
        .unwrap();
    });

    info!("主线程等待退出命令");

    main_recv.recv().await;

    info!("主线程退出");

    Ok(())
}
