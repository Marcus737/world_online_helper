use std::thread;

use anyhow::Result;
use tracing::{error, info};

use crate::funs::GameHelper;

mod config_util;
mod funs;
mod mumu_manager;
mod ocr_helper;
mod util;

const CHANNLE_MAX_MSG_SIZE: usize = 100;

#[derive(Debug, PartialEq, Clone, Copy)]
enum Msg {
    Exit,
    ClearBag,
    ClickTaskButton,
    AutoFight,
    AutoClickTaskButton,
}

#[tokio::main]
async fn main() -> Result<()> {
    util::init_logger();

    let app_config = config_util::AppConfig::load_from_file()?;

    info!("start adb server");
    util::run_command(&app_config.adb_path, vec!["start-server"])?;

    info!("start ocr server");
    let ocr_server = ocr_helper::OcrServer::launch()?;

    let (win_width, win_height) = &app_config.vm_client_window_size;
    let (x, y) = &app_config.first_vm_client_pos;
    let mut x2 = *x;
    let client_num = *&app_config.vm_client_num;

    let (main_send, mut main_recv) = tokio::sync::mpsc::channel::<Msg>(CHANNLE_MAX_MSG_SIZE);
    let mut sender_vec = vec![];

    for i in 0..client_num {
        let vm_client = mumu_manager::VmClient::new(i, &app_config.manager_path);
        let mut game_helper = GameHelper::new(
            vm_client,
            ocr_server.get_client().await?,
            Some(&app_config.app_package_names[i]),
        )
        .await?;
        game_helper.vm_client.set_layout_window(
            Some(x2),
            Some(*y),
            Some(*win_width),
            Some(*win_height),
        )?;
        x2 += win_width;

        let (send, mut recv) = tokio::sync::mpsc::channel::<Msg>(CHANNLE_MAX_MSG_SIZE);
        sender_vec.push(send);

        tokio::spawn(async move {
            info!("异步任务{}已开启", i);
            let mut auto_click_task_button_on = true;
            loop {
                if let Some(msg) = recv.recv().await {
                    match msg {
                        Msg::Exit => break,
                        Msg::ClearBag => {
                            if let Err(e) = game_helper.clear_bag_v3().await {
                                error!("{} ClearBag error:{}", i, e)
                            };
                        }
                        Msg::ClickTaskButton => {
                            if let Err(e) = game_helper.adb_device.keyevent(12).await {
                                error!("点击任务按钮失败:{}", e);
                            };
                        }
                        Msg::AutoFight => {
                            if let Err(e) = game_helper.adb_device.tap(480, 1200).await {
                                error!("点击自动战斗失败:{}", e);
                            }
                        }
                        Msg::AutoClickTaskButton => {
                            if let Err(e) =
                                game_helper.auto_click_task(auto_click_task_button_on).await
                            {
                                error!("自动点击任务失败:{}", e);
                            }
                            auto_click_task_button_on = !auto_click_task_button_on;
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
                        send_all(&sender_vec, Msg::AutoClickTaskButton);
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
