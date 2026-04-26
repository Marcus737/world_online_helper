use std::{
    collections::HashSet,
    env,
    fmt::Debug,
    time::{Duration, SystemTime},
};

use crate::{
    config_util, mumu_manager::VmClient, orc_helper::OcrClient, util::{self, ImageHelper, Point}
};
use anyhow::{Result, anyhow};
use droidrun_adb::AdbServer;
use image::{DynamicImage, GenericImage};
use thiserror::Error;
use tracing::{debug, info};


fn get_bag_grid_center_pos_vec() -> Vec<Point> {
    let point = &config_util::GAME_HELPER_CONFIG.bag_first_grid_center_pos;
    let size = &config_util::GAME_HELPER_CONFIG.bag_grid_size;
    let (mut x, mut y) = (point.x, point.y);
    let mut v = vec![];

    for _ in 0..4 {
        for _ in 0..8 {
            v.push(Point::new(x, y));
            x += size.width as i32;
        }
        y += size.height as i32;
        x = point.x as i32;
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

pub fn get_current_item_info_v2(path: &str) -> Result<ItemInfo> {
    debug!("path:{}", path);

    let start = SystemTime::now();
    let output = util::run_command_with_work_dir(
        "uv",
        r"C:\Users\10401\Desktop\rust_projects\shi_jie_ol_helper_v3\py_ocr\",
        vec!["run", "main.py", path],
    )?;
    info!(
        "ocr use time: {:?}",
        SystemTime::now().duration_since(start)
    );
    let ocr_result = output.std_out_str;
    let ocr_result = ocr_result.replace(" ", "");
    debug!("ocr_result: {}", ocr_result);

    let info = ItemInfo::new(ocr_result);
    Ok(info)
}

fn need_remove(
    info: &ItemInfo,
    rarity_filter: &[Rarity],
    item_type_filter: &[ItemType],
    name_filter: &[&str],
) -> bool {
    //在名单上的直接删除
    if name_filter
        .into_iter()
        .any(|name| info.ocr_res.contains(name))
    {
        return true;
    }

    //根据稀有度和装备删除
    if let (Some(rarity), Some(item_type)) = (&info.rarity, &info.item_type) {
        let raity_exist = rarity_filter.into_iter().any(|r| r == rarity);
        let item_type_exist = item_type_filter.into_iter().any(|it| it == item_type);
        return raity_exist && item_type_exist;
    }

    false
}

#[derive(Debug, PartialEq)]
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

#[derive(Debug, PartialEq)]
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
        })
    }

    async fn remove_item(&mut self, input: &DynamicImage) -> Result<()> {
        debug!("{} 正在删除", self.vm_client.get_vm_index());
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
        debug!("ocr items:{:?}", items);
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
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    async fn handle_empty_grid(
        &mut self,
        set: &mut HashSet<(i32, i32)>,
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str],
    ) -> Result<()> {
        //还可以优化，直接新旧两个图对比，精准找出不同位置

        let mut need_remove_pos_vec = vec![];
        //遍历之前的每个空格子
        for pos in set.iter() {
            let (x, y) = pos;
            
            self.click_blank_pos_for_close().await?;

            //点击格子
            self.adb_device.tap(*x, *y).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;

            //截图
            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;

            //如果是空格子就跳过
            if let None = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "温馨提示2")?
            {
                continue;
            }

            //获取格子信息
            let path = save_detail_rect_img(&input_img, self.vm_client.get_vm_index())?;
            let item_info = get_current_item_info_v2(&path)?;

            //是否是可删除的
            if need_remove(&item_info, rarity_filter, item_type_filter, name_filter) {
                self.remove_item(&input_img).await?;
                continue;
            }

            //如果当前格子是消耗品就使用
            if let Some(LockAttr::UnLocked) = item_info.lock_attr {
                //点击开启
                if let Some(point) = self
                    .image_helper
                    .get_template_img_pos_by_name(&input_img, "按钮_使用2")?
                {
                    self.adb_device
                        .tap(point.center_x as i32, point.center_y as i32)
                        .await?;
                    //等待反应
                    tokio::time::sleep(Duration::from_millis(300)).await;

                    //点击空白位置关闭界面
                    self.click_blank_pos_for_close().await?;

                    //再次点击格子
                    self.adb_device.tap(*x, *y).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    //再次判断，这次一定不会是消耗品
                    //截图
                    let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;

                    //如果是空格子就跳过
                    if let None = self
                        .image_helper
                        .get_template_img_pos_by_name(&input_img, "温馨提示2")?
                    {
                        continue;
                    }

                    //获取格子信息
                    let path = save_detail_rect_img(&input_img, self.vm_client.get_vm_index())?;
                    let item_info = get_current_item_info_v2(&path)?;

                    //是否是可删除的
                    if need_remove(&item_info, rarity_filter, item_type_filter, name_filter) {
                        self.remove_item(&input_img).await?;
                        continue;
                    }
                } else {
                    return Err(anyhow!("找不到使用按钮"));
                }

                //当前位置是不变物品
                need_remove_pos_vec.push(*pos);
            }
        }
        //去除不可变物品位置
        need_remove_pos_vec.into_iter().for_each(|e| {
            set.remove(&e);
        });

        Ok(())
    }

    //自动打开箱子并清理背包
    pub async fn clear_bag_v2(
        &mut self,
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str],
    ) -> anyhow::Result<()> {
        let mut empty_grid_set = HashSet::new();
        let pos_vec = get_bag_grid_center_pos_vec();
        let mut i = 0;
        loop {
            if i >= pos_vec.len() {
                break;
            }

            let Point{x, y} = pos_vec[i];
            //下一格子
            i += 1;

            //点击空白位置关闭界面
            self.click_blank_pos_for_close().await?;

            //点击当前格子
            self.adb_device.tap(x as i32, y as i32).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;

            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            //如果是空格子就跳过
            if let None = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "温馨提示2")?
            {
                debug!("当前格子为空");
                empty_grid_set.insert((x, y));
                continue;
            }

            //获取格子信息
            let path = save_detail_rect_img(&input_img, self.vm_client.get_vm_index())?;
            let item_info = get_current_item_info_v2(&path)?;
            debug!("item_info：{:?}", item_info);

            //如果当前格子是消耗品就使用
            if let Some(LockAttr::UnLocked) = item_info.lock_attr {
                debug!("当前格子为消耗品");
                //点击开启
                if let Some(point) = self
                    .image_helper
                    .get_template_img_pos_by_name(&input_img, "按钮_使用2")?
                {
                    self.adb_device
                        .tap(point.center_x as i32, point.center_y as i32)
                        .await?;
                    //等待反应
                    tokio::time::sleep(Duration::from_millis(300)).await;

                    //前面的空格子可能有物品
                    self.handle_empty_grid(
                        &mut empty_grid_set,
                        rarity_filter,
                        item_type_filter,
                        name_filter,
                    )
                    .await?;

                    //再次判断当前位置
                    i -= 1;
                } else {
                    return Err(anyhow!("找不到使用按钮"));
                }
            }

            //是否是可删除的
            if need_remove(&item_info, rarity_filter, item_type_filter, name_filter) {
                debug!("当前格子可移除");
                self.remove_item(&input_img).await?;
                //当前格子为空，加入空格子列表
                empty_grid_set.insert((x, y));
                continue;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use tracing::info;

    use crate::{
        funs::{GameHelper, ItemType, Rarity},
        mumu_manager::VmClient,
        orc_helper::OcrClient,
        util,
    };
    const FILTER_NAMES: [&str; 43] = [
        "防具箱（锁）",
        "服饰箱（锁）",
        "防具箱（锁）",
        "武器箱（锁）",
        "秘宝礼包（锁）",
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
        "冰雪披风",
        "美女节时装（2天）",
        "黑魔一族护符（1天）",
        "屠龙勇士称号",
    ];
    const MANAGER_PATH: &str = r"c:\Users\10401\software\MuMuPlayer\nx_main\MuMuManager.exe";
    const OCR_SERVER_ADDR: &str = "127.0.0.1:9000";

    #[tokio::test]
    async fn test_new_game_helper() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR), None)
            .await
            .unwrap();
        info!("{:?}", gh);
    }

    #[tokio::test]
    async fn test_clear_bag_v2() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let mut gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR), None)
            .await
            .unwrap();
        info!("{:?}", gh);
        gh.clear_bag_v2(
            &[Rarity::Common, Rarity::Fine],
            &[ItemType::Equipment],
            &FILTER_NAMES,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn screenshot() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR), None)
            .await
            .unwrap();
        let data = gh.adb_device.screencap().await.unwrap();
        image::load_from_memory(&data)
            .unwrap()
            .save("test_scfeenshot.png")
            .unwrap();
    }
}
