use std::{
    env, fmt::Debug, path::Path, time::{Duration, SystemTime}
};

use crate::{
    config_util, mumu_manager::VmClient, orc_helper::OcrClient, util::{ImageHelper, Point}
};
use anyhow::{Result, anyhow};
use droidrun_adb::AdbServer;
use image::{DynamicImage, GenericImage, GenericImageView};
use serde::Deserialize;
use thiserror::Error;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

// static BAG_GRID_CENTER_POS_VEC:LazyLock<Vec<Point>> = LazyLock::new(|| get_bag_grid_center_pos_vec());

fn get_bag_grid_center_pos_vec() -> Vec<Point> {
    let point = &config_util::GAME_HELPER_CONFIG.bag_first_grid_center_pos;
    let size = &config_util::GAME_HELPER_CONFIG.bag_grid_size;
    let (mut x, mut y) = (point.x, point.y);
    let mut v = vec![];

    for _ in 0..4 {
        for _ in 0..8 {
            v.push(Point::new(x, y));
            x += size.width;
        }
        y += size.height;
        x = point.x;
    }
    v.pop();
    v.pop();
    v
}

fn save_detail_rect_img(input: &DynamicImage, id: String) -> Result<String> {
    let mut gray_img = input.to_luma8();

    let th = 130;
    let (mut min_x, mut max_x) = (gray_img.width(), 0);
    let (mut min_y, mut max_y) = (gray_img.height(), 0);

    let mut find = false;
    for (x, y, p) in gray_img.enumerate_pixels() {
        if p.0[0] > th {
            find = true;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    if find {
        let sub_img = gray_img.sub_image(min_x, min_y, max_x - min_x, max_y - min_y);
        let path = env::current_dir()?.join(format!("sub_image_vm_index{}.png", id));
        sub_img.to_image().save(&path)?;
        return Ok(path.display().to_string());
    } else {
        return Err(anyhow!("not found sub rect"));
    }
}

fn need_remove(
    info: &ItemInfo
) -> bool {
    let name_filter = &config_util::GAME_HELPER_CONFIG.remove_item_names;

    //在名单上的直接删除
    if name_filter
        .into_iter()
        .any(|name| info.ocr_res.contains(name))
    {
        return true;
    }

    let rarity_filter = &config_util::GAME_HELPER_CONFIG.remove_item_raritys;
    let item_type_filter = &config_util::GAME_HELPER_CONFIG.remove_item_types;
    //根据稀有度和装备删除
    if let (Some(rarity), Some(item_type)) = (&info.rarity, &info.item_type) {
        let raity_exist = rarity_filter.into_iter().any(|r| r == rarity);
        let item_type_exist = item_type_filter.into_iter().any(|it| it == item_type);
        return raity_exist && item_type_exist;
    }

    false
}

#[derive(Debug, PartialEq, Deserialize)]
pub enum ItemType {
    //装备
    Equipment,
    //任务物品
    QuestItem,
    //道具消耗品
    Consumable,
    //建筑材料
    BuildingMaterials,
    //特殊
    SpecialItem,
    //宠物
    Pet,
}

#[derive(Error, Debug, PartialEq)]
pub enum ParseItemTypeError {
    #[error("无效的装备字符串: {0}")]
    InvalidItemType(String),
}

impl TryFrom<&str> for ItemType {
    type Error = ParseItemTypeError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let vector= &config_util::GAME_HELPER_CONFIG.equipment_names;

        let it = match s {
            s if s.contains("任务物品") => Self::QuestItem,
            s if s.contains("道具") => Self::Consumable,
            s if s.contains("建筑材料") => Self::BuildingMaterials,
            s if s.contains("特殊") => Self::SpecialItem,
            s if s.contains("宠") => Self::Pet,
            s if vector.iter().any(|name| s.contains(name)) => Self::Equipment,
            _ => return Err(ParseItemTypeError::InvalidItemType(s.to_string())),
        };

        Ok(it)
    }
}

#[derive(Debug, PartialEq, Deserialize)]
pub enum Rarity {
    //普通
    Common,
    //精致
    Fine,
    //稀有
    Rare,
    //史诗
    Epic,
    //传说
    Legendary,
}

// 使用 thiserror 定义错误类型
#[derive(Error, Debug, PartialEq)]
pub enum ParseRarityError {
    #[error("无效的稀有度字符串: {0}")]
    InvalidRarity(String),
}

impl TryFrom<&str> for Rarity {
    type Error = ParseRarityError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            s if s.contains("普通") => Ok(Rarity::Common),
            s if s.contains("精致") => Ok(Rarity::Fine),
            s if s.contains("稀有") => Ok(Rarity::Rare),
            s if s.contains("史诗") => Ok(Rarity::Epic),
            s if s.contains("传说") => Ok(Rarity::Legendary),
            _ => Err(ParseRarityError::InvalidRarity(s.to_string())),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum LockAttr {
    Locked,
    UnLocked,
}

impl LockAttr {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            s if s.contains("需要万能钥匙") => Some(LockAttr::Locked),
            s if s.contains("直接开启") => Some(LockAttr::UnLocked),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ItemInfo {
    //类型
    pub item_type: Option<ItemType>,
    //品质
    pub rarity: Option<Rarity>,
    //是否是自动绑定，自动绑定不能交易
    pub linked: bool,
    //是否是锁住的，锁住需要钥匙
    pub lock_attr: Option<LockAttr>,
    pub ocr_res: String,
}

impl ItemInfo {
    pub fn new(ocr_result: String) -> Self {
        let item_type_opt = if let Ok(it) = ItemType::try_from(ocr_result.as_str()) {
            Some(it)
        } else {
            None
        };

        let rarity_opt = if let Ok(r) = Rarity::try_from(ocr_result.as_str()) {
            Some(r)
        } else {
            None
        };

        let linked = ocr_result.contains("自动绑定");

        let lock_attr = LockAttr::from_str(ocr_result.as_str());

        ItemInfo {
            item_type: item_type_opt,
            rarity: rarity_opt,
            linked,
            lock_attr,
            ocr_res: ocr_result,
        }
    }
}

#[derive(Debug)]
pub struct GameHelper {
    pub vm_client: VmClient,
    image_helper: ImageHelper,
    ocr_client: OcrClient,
    pub adb_device: droidrun_adb::AdbDevice,
    auto_click_task_handle: Option<JoinHandle<()>>
}

impl GameHelper {
    pub async fn new(
        vm_client: VmClient,
        ocr_client: OcrClient,
        app_package_name: Option<&str>,
    ) -> Result<Self> {
        vm_client.lanch_vm(app_package_name)?;
        vm_client
            .wait_for_launch_finsh_async(Duration::from_secs(60))
            .await?;
        info!("vm_client_{} started", vm_client.get_vm_index());

        let vm_info = vm_client.get_vm_info()?;
        let ip = vm_info.adb_host_ip.ok_or(anyhow!("ip is none"))?;
        let port = vm_info.adb_port.ok_or(anyhow!("port is none"))?;
        let addr = format!("{}:{}", ip, port);
        info!("addr:{}", addr);

        let server = AdbServer::default();
        server.connect_device(&addr).await?;

        info!("connected device");

        let adb_device = server.device_by_serial(&addr).await?;

        Ok(Self {
            vm_client,
            image_helper: ImageHelper::new()?,
            adb_device,
            ocr_client,
            auto_click_task_handle: None
        })
    }


    async fn remove_item(&mut self, input: &DynamicImage) -> Result<()> {
        debug!("{} 正在删除", self.vm_client.get_vm_index());
        // if let None= self.image_helper.get_template_img_pos_by_name(input, "温馨提示2")? {
        //     return Err(anyhow!("当前界面不是详情界面"));
        // }
        //找出售按钮
        if let Some(point) = self
            .image_helper
            .get_template_img_pos_by_name(input, "按钮_出售2")?
        {
            self.adb_device
                .tap(point.center_x as i32, point.center_y as i32)
                .await?;
            //等待点击生效
            tokio::time::sleep(Duration::from_millis(200)).await;
        } else {
            return Err(anyhow!("没有找到出售按钮"));
        }
        //找确定按钮
        let input = image::load_from_memory(&self.adb_device.screencap().await?)?;
        if let Some(point) = self
            .image_helper
            .get_template_img_pos_by_name(&input, "按钮_确定2")?
        {
            self.adb_device
                .tap(point.center_x as i32, point.center_y as i32)
                .await?;
            //等待点击生效
            tokio::time::sleep(Duration::from_millis(200)).await;
        } else {
            return Err(anyhow!("没有找到确定按钮"));
        }
        Ok(())
    }

    pub async fn get_current_item_info_v3(&self, path: &str) -> Result<ItemInfo> {
        debug!("path:{}", path);

        let start = SystemTime::now();
        let items = self.ocr_client.recognize(path).await?;
        // debug!("ocr items:{:?}", items);
        info!(
            "ocr use time: {:?}",
            SystemTime::now().duration_since(start)
        );
        let texts: String = items.into_iter().map(|it| it.text).collect();
        let info = ItemInfo::new(texts);
        Ok(info)
    }

    async fn click_blank_pos_for_close(&mut self) -> anyhow::Result<()> {
        let point = &config_util::GAME_HELPER_CONFIG.back_pos;
        //点击空白位置关闭界面
        self.adb_device.tap(point.x as i32, point.y as i32).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(())
    }

    

    pub async fn handle_change_grid(&mut self, bag_img: DynamicImage) -> anyhow::Result<()> {
        //先确保当前界面是背包界面
        // self.image_helper.loop_find_image(
        //     "背包_移动仓库2", 
        //     Duration::from_secs(5),
        //     || async  {
        //         let point = &config_util::GAME_HELPER_CONFIG.back_pos;
        //         //点击空白位置关闭界面
        //         self.adb_device.tap(point.x as i32, point.y as i32).await?;
        //         tokio::time::sleep(Duration::from_millis(300)).await;
        //         Ok(image::load_from_memory(&self.adb_device.screencap().await?)?)
        //     }
        // )
        // .await?
        // .ok_or(anyhow!("找不到背包界面"))?;

        //截图
        let new_bag_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
        let pos_vec = get_bag_grid_center_pos_vec();

        let cmp_len = 20;
        let th = 0.8;
        //对比这些格子周围的像素 20 * 20
        for Point { x, y } in pos_vec {
            let mut same_cnt = 0;
            for i in 0..cmp_len {
                for j in 0..cmp_len {
                    let pi = x as u32 + i;
                    let pj = y as u32 + j;
                    if bag_img.get_pixel(pi, pj).eq(&new_bag_img.get_pixel(pi, pj)) {
                        same_cnt += 1;
                    }
                }
            }
            if (same_cnt as f32 / (cmp_len * cmp_len) as f32) < th {
                //当前格子内容变化了
                self.clear_bag_v3_inner(x, y).await?;
            }
        }

        Ok(())
    }

    async fn clear_bag_v3_inner(&mut self, x: i32, y: i32)-> Result<()> {
        //点击空白关闭界面
        self.click_blank_pos_for_close().await?;

        //先截图背包界面
        let old_bag_img = image::load_from_memory(&self.adb_device.screencap().await?)?;

        //点击当前格子
        self.adb_device.tap(x, y).await?;
        //休眠时间要大，不然会把箭头截进去
        tokio::time::sleep(Duration::from_millis(400)).await;

        //背包格子详情
        let bag_grid_img = image::load_from_memory(&self.adb_device.screencap().await?)?;

        //只要详细信息的部分部分
        let path = save_detail_rect_img(&bag_grid_img, self.vm_client.get_vm_index())?;
        let item_info = self.get_current_item_info_v3(&path).await?;
        debug!("item_info：{:?}", item_info);

        //如果是空格子就跳过
        if let None = self.image_helper.get_template_img_pos_by_name(&bag_grid_img, "温馨提示2")? {
            debug!("当前格子为空");
            self.click_blank_pos_for_close().await?;
            return Ok(());
        };


        //是否是可删除的
        if need_remove(&item_info) {
            debug!("当前格子可移除");
            // empty_grid_set.insert(Point::new(x, y));
            self.remove_item(&bag_grid_img).await?;
            self.click_blank_pos_for_close().await?;
            return Ok(());
        }

        //如果当前格子是消耗品就使用
        if let Some(LockAttr::UnLocked) = item_info.lock_attr {
            debug!("当前格子为消耗品");
            //点击开启
            if let Some(point) = self
                .image_helper
                .get_template_img_pos_by_name(&bag_grid_img, "按钮_使用2")?
            {
                self.adb_device
                    .tap(point.center_x as i32, point.center_y as i32)
                    .await?;
                //等待反应
                tokio::time::sleep(Duration::from_millis(300)).await;
                self.click_blank_pos_for_close().await?;


                // 【唯一修改点】：在这里使用 Box::pin 打破交叉递归
                Box::pin(self.handle_change_grid(old_bag_img)).await?;
                return Ok(());
            } else {
                return Err(anyhow!("找不到使用按钮"));
            }
        }

        self.click_blank_pos_for_close().await?;

        Ok(())
    }

    pub async fn clear_bag_v3(&mut self) -> anyhow::Result<()>{
        //点击背包1
        let bag1_pos = &config_util::GAME_HELPER_CONFIG.bag_1_pos;
        self.adb_device.tap(bag1_pos.x, bag1_pos.y).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        for p in get_bag_grid_center_pos_vec() {
             self.clear_bag_v3_inner(p.x, p.y).await?
        }

        self.click_blank_pos_for_close().await?;

        let bag2_pos = &config_util::GAME_HELPER_CONFIG.bag_2_pos;
        self.adb_device.tap(bag2_pos.x, bag2_pos.y).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        for Point{x, y} in get_bag_grid_center_pos_vec() {
             self.clear_bag_v3_inner(x, y).await?
        }

        Ok(())
    }


    pub async fn auto_click_task(&mut self, on: bool) -> Result<()>{
        if !on {
            if let Some(join_handle) = self.auto_click_task_handle.take() {
                join_handle.abort();
                info!("已关闭自动点击任务");
            }
            return Ok(());
        }

        //判断是否有组队
        let queue_button_pos = (135, 230);
        self.adb_device.tap(queue_button_pos.0, queue_button_pos.1).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let input = image::load_from_memory(&self.adb_device.screencap().await?)?;

        if let Some(_) = self.image_helper.get_template_img_pos_by_name(&input, "离开队伍")? {
            info!("当前是队员状态，不需要自动点击任务");
            return Ok(());
        }

        
        let adb_client_clone = self.adb_device.clone();
        let handle = tokio::spawn(async move {
            let target_img = match image::open(Path::new("res/任务按钮存在.png")) {
                Ok(img) => img,
                Err(e) => {
                    error!("加载任务存在按钮图片失败:{}", e);
                    return;
                },
            };
            let threshold = 0.8;
            loop {
                let data = match adb_client_clone.screencap().await {
                    Ok(data) => data,
                    Err(e) => {
                        error!("获取屏幕截图失败:{}", e);
                        return ;
                    },
                };

                let input_img = match image::load_from_memory(&data) {
                    Ok(img) => img,
                    Err(e) => {
                        error!("从内存加载图片失败:{}", e);
                        return ;
                    },
                };
                //对比图片
                let height = 60;
                let input_img_height = input_img.height();
                let start_j =input_img_height / 2;

                let mut same_cnt = 0;
                for j in start_j..input_img_height - height {
                    for k in 0..height {
                        if input_img.get_pixel(150, j + k).eq(&target_img.get_pixel(0, k)) {
                            same_cnt += 1;
                        }
                    }
                    let confidence = same_cnt as f32 / height as f32;
                    // debug!("confidence:{}", confidence);
                    if confidence > threshold {
                        //屏幕存在任务按钮，发送事件
                        info!("屏幕存在任务按钮，点击");
                        if let Err(e) = adb_client_clone.keyevent(12).await {
                            error!("发送输入事件失败:{}", e);
                            return ;
                        };
                        break;
                    }
                }

                tokio::time::sleep(Duration::from_millis(300)).await;
            }

        });
        self.auto_click_task_handle = Some(handle);
        info!("已开启自动点击任务");
        Ok(())
    }
}

impl Drop for GameHelper {
    fn drop(&mut self) {
        if let Some(handle) = self.auto_click_task_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod test {
    use image::GenericImage;
    use tracing::info;

    use crate::{
        config_util, funs::GameHelper, mumu_manager::VmClient, orc_helper::OcrClient, util
    };


    #[tokio::test]
    async fn test_new_game_helper() {
        util::init_logger();
        
        let vm_client = VmClient::new(0, &config_util::APP_CONFIG.manager_path);
        let gh = GameHelper::new(
                vm_client, 
                OcrClient::new(&format!("127.0.0.1:{}", config_util::OCR_CONFIG.server_port)), 
                None
            )
            .await
            .unwrap();
        info!("{:?}", gh);
    }

    #[tokio::test]
    async fn test_clear_bag_v3() {
        util::init_logger();
        let vm_client = VmClient::new(0, &config_util::APP_CONFIG.manager_path);
        let server_addr = format!("127.0.0.1:{}", config_util::OCR_CONFIG.server_port);
        let mut gh = GameHelper::new(vm_client, OcrClient::new(&server_addr), None)
            .await
            .unwrap();
        info!("{:?}", gh);
        gh.clear_bag_v3()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn screenshot() {
        util::init_logger();
        let vm_client = VmClient::new(0, &config_util::APP_CONFIG.manager_path);
        let server_addr = format!("127.0.0.1:{}", config_util::OCR_CONFIG.server_port);
        let gh = GameHelper::new(vm_client, OcrClient::new(&server_addr), None)
            .await
            .unwrap();
        let data = gh.adb_device.screencap().await.unwrap();
        let mut img = image::load_from_memory(&data)
            .unwrap();

        img.sub_image(150, 680, 1, 90).to_image().save("任务按钮存在.png").unwrap();

        img.save("test_scfeenshot.png").unwrap();
    }
}
