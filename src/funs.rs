use std::{
    collections::HashSet, env, fmt::Debug, path::Path, result, time::{Duration, SystemTime}
};

use crate::{
    mumu_manager::VmClient, orc_helper::OcrClient, util::{self, ImageHelper}
};
use anyhow::{Result, anyhow};
use droidrun_adb::AdbServer;
use image::{DynamicImage, GenericImage};
use thiserror::Error;
use tracing::{debug, info};

const TASK_BUTTON_NAME_VEC: [&str; 6] = [
    "任务按钮_任务寻路",
    "任务按钮_完成任务",
    "任务按钮_接受任务",
    "任务按钮_确认",
    "任务按钮_继续",
    "任务按钮_离开",
];

const TASK_BUTTON_NAME_VEC2: [&str; 5] = [
    "任务按钮_任务寻路2",
    "任务按钮_完成任务2",
    "任务按钮_接受任务2",
    "任务按钮_确认2",
    "任务按钮_继续2",
];

const EQUIPMENT_NAME_VEC: [&str; 26] = [
    "链",
    "戒",
    "时装",
    "衣",
    "护手",
    "鞋",
    "裤",
    "腰带",
    "背",
    "头",
    "肩",
    "护符",
    "坐骑",
    "副手",
    "单手刀",
    "单手剑",
    "双手刀",
    "双手剑",
    "法器",
    "法杖",
    "轻弩",
    "重型武器",
    "重弩",
    "长柄武器",
    "弓箭",
    "杖"
];

const BAG_BUTTON_POS: [i32; 2] = [320, 1220];
const BAG_FIRST_GRID_CENTER_POS: [i32; 2] = [50, 810];
const BAG_GRID_WIDTH: i32 = 90;
const BAG_GRID_HIEGHT: i32 = 90;
const BACK_POS: [i32; 2] = [320, 40];
const BAG_POS_1 : [i32; 2] = [80, 670];
const BAG_POS_2 : [i32; 2] = [200, 670];

fn get_bag_grid_center_pos_vec() -> Vec<(i32, i32)> {
    let mut v = vec![];
    let mut current = (BAG_FIRST_GRID_CENTER_POS[0], BAG_FIRST_GRID_CENTER_POS[1]);
    for _ in 0..4 {
        for _ in 0..8 {
            v.push(current);
            current.0 += BAG_GRID_WIDTH;
        }
        current.1 += BAG_GRID_HIEGHT;
        current.0 = BAG_FIRST_GRID_CENTER_POS[0]
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




pub fn get_current_item_info(path: &str) -> Result<ItemInfo> {
    let ocr_result = win_ocr::ocr_with_lang(path, "zh-cn")?;
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
    //宝石
    Gem,
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

        let it = match s {
            s if s.contains("任务物品") => Self::QuestItem,
            s if s.contains("道具") => Self::Consumable,
            s if s.contains("建筑材料") => Self::BuildingMaterials,
            s if s.contains("特殊") => Self::SpecialItem,
            s if s.contains("宠") => Self::Pet,
            s if EQUIPMENT_NAME_VEC.iter().any(|name| s.contains(name)) => Self::Equipment,
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
    UnLocked
}

impl LockAttr {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            s if s.contains("需要万能钥匙") => {
                Some(LockAttr::Locked)
            },
            s if s.contains("直接开启") => {
                Some(LockAttr::UnLocked)
            },
            _ => None
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
    pub async fn new(vm_client: VmClient, ocr_client: OcrClient, app_package_name: Option<&str>) -> Result<Self> {
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
            ocr_client
        })
    }


    pub async fn click_task_button(&mut self) -> Result<()> {
        let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
        input_img
            .save(format!("{}.png", self.vm_client.get_vm_index()))
            .unwrap();
        for name in TASK_BUTTON_NAME_VEC2 {
            if let Some(p) = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, name)?
            {
                debug!("{} found ponit: {:?}", self.vm_client.get_vm_index(), p);
                self.adb_device
                    .tap(p.center_x as i32, p.center_y as i32)
                    .await?;
                break;
            }
        }
        // self.image_helper.get_template_img_pos(input, template, method);
        Ok(())
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

    pub async  fn get_current_item_info_v3(&self, path: &str) -> Result<ItemInfo> {
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

    pub async fn sale_bag_items(
        &mut self,
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str],
    ) -> Result<bool> {
        //点击背包
        self.adb_device
            .tap(BAG_BUTTON_POS[0], BAG_BUTTON_POS[1])
            .await?;
        //等待反应
        tokio::time::sleep(Duration::from_millis(200)).await;

        //判断是否在背包界面
        {
            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            if let None = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "背包_移动仓库2")?
            {
                return Err(anyhow!("当前界面不是背包界面"));
            }
            //点击背包整理
            if let Some(point) = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "按钮_背包整理2")?
            {
                self.adb_device
                    .tap(point.center_x as i32, point.center_y as i32)
                    .await?;
                //等待点击生效
                tokio::time::sleep(Duration::from_millis(400)).await;
            } else {
                return Err(anyhow!("按钮_背包整理"));
            }
        }


        //点击背包1
        self.adb_device
            .tap(BAG_POS_1[0], BAG_POS_1[1])
            .await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut has_sold = self.sale_bag_items_inner(rarity_filter, item_type_filter, name_filter).await?;
        debug!("背包1是否售出:{}", has_sold);

        //点击背包2
        self.adb_device
            .tap(BAG_POS_2[0], BAG_POS_2[1])
            .await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        has_sold |= self.sale_bag_items_inner(rarity_filter, item_type_filter, name_filter).await?;
        debug!("背包2是否售出:{}", has_sold);
        Ok(has_sold)
    }

    async fn sale_bag_items_inner(
        &mut self,
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str],
    ) -> Result<bool> {

        let mut has_sold = false;
        //获取每个背包格子中心点
        let grid_pos_vec = get_bag_grid_center_pos_vec();
        for (x, y) in grid_pos_vec {
            //点击格子
            self.adb_device.tap(x, y).await?;
            //等待反应
            tokio::time::sleep(Duration::from_millis(300)).await;
            //找详情标志
            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            if let None = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "温馨提示2")?
            {
                info!("找不到详情面板，当前格子可能为空");
                continue;
            }
            //获取详情面板矩阵
            let path = save_detail_rect_img(&input_img, self.vm_client.get_vm_index())?;
            //文字识别

            let item_info = self.get_current_item_info_v3(&path).await?;
            info!("item_info:\n{:?}", item_info);

            //判断是否需要删除
            if need_remove(&item_info, rarity_filter, item_type_filter, name_filter) {
                //删除操作
                self.remove_item(&input_img).await?;
                has_sold = true;
            } else {
                info!("不需要删除");
            }

            //点击关闭详情界面
            self.adb_device.tap(BACK_POS[0], BACK_POS[1]).await?;

            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        Ok(has_sold)
    }

    async fn open_bag_item_inner(
        &mut self,         
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str]
    ) -> anyhow:: Result<bool> {
        let mut has_new_item = false;
        let grid_pos_vec = get_bag_grid_center_pos_vec();
        for (x, y) in grid_pos_vec {
            loop {
                 //点击格子
                self.adb_device.tap(x, y).await?;
                //等待反应
                tokio::time::sleep(Duration::from_millis(300)).await;
                //找详情标志
                let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
                if let None = self
                    .image_helper
                    .get_template_img_pos_by_name(&input_img, "温馨提示2")?
                {
                    info!("找不到详情面板，当前格子可能为空");
                    break;
                }
                //获取详情面板矩阵
                let path = save_detail_rect_img(&input_img, self.vm_client.get_vm_index())?;
                //文字识别

                let item_info = self.get_current_item_info_v3(&path).await?;
                info!("item_info:\n{:?}", item_info);

                //判断是否是可直接开启的
                if let Some(LockAttr::UnLocked) = item_info.lock_attr {
                        //点击开启
                        if let Some(point) = self
                            .image_helper
                            .get_template_img_pos_by_name(&input_img, "按钮_使用2")?
                        {
                            has_new_item = true;
                            self.adb_device.tap(point.center_x as i32, point.center_y as i32).await?;
                            //等待反应
                            tokio::time::sleep(Duration::from_millis(300)).await;                
                        }else {
                            return Err(anyhow!("找不到使用按钮"));
                        }
                }else {
                    //判断是否需要删除
                    if need_remove(&item_info, rarity_filter, item_type_filter, name_filter) {
                        self.remove_item(&input_img);
                    }
                    break;
                }
                //点击关闭界面
                self.adb_device.tap(BACK_POS[0], BACK_POS[1]).await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
           

            //点击关闭界面
            self.adb_device.tap(BACK_POS[0], BACK_POS[1]).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Ok(has_new_item)
    } 

    async fn open_bag_item(
        &mut self,         
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str]
    ) -> anyhow::Result<bool> {
        //点击背包
        self.adb_device
            .tap(BAG_BUTTON_POS[0], BAG_BUTTON_POS[1])
            .await?;
        //等待反应
        tokio::time::sleep(Duration::from_millis(200)).await;

        //判断是否在背包界面
        {
            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            if let None = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "背包_移动仓库2")?
            {
                return Err(anyhow!("当前界面不是背包界面"));
            }
            //点击背包整理
            if let Some(point) = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "按钮_背包整理2")?
            {
                self.adb_device
                    .tap(point.center_x as i32, point.center_y as i32)
                    .await?;
                //等待点击生效
                tokio::time::sleep(Duration::from_millis(400)).await;
            } else {
                return Err(anyhow!("按钮_背包整理"));
            }
        }


        //点击背包
        self.adb_device
            .tap(BAG_BUTTON_POS[0], BAG_BUTTON_POS[1])
            .await?;
        //等待反应
        tokio::time::sleep(Duration::from_millis(200)).await;

        //判断是否在背包界面
        {
            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            if let None = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "背包_移动仓库2")?
            {
                return Err(anyhow!("当前界面不是背包界面"));
            }
            //点击背包整理
            if let Some(point) = self
                .image_helper
                .get_template_img_pos_by_name(&input_img, "按钮_背包整理2")?
            {
                self.adb_device
                    .tap(point.center_x as i32, point.center_y as i32)
                    .await?;
                //等待点击生效
                tokio::time::sleep(Duration::from_millis(400)).await;
            } else {
                return Err(anyhow!("按钮_背包整理"));
            }
        }

        //点击背包1
        self.adb_device
            .tap(BAG_POS_1[0], BAG_POS_1[1])
            .await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut has_new_item = self.open_bag_item_inner(rarity_filter, item_type_filter, name_filter).await?;

        //点击背包2
        self.adb_device
            .tap(BAG_POS_2[0], BAG_POS_2[1])
            .await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        has_new_item |= self.open_bag_item_inner(rarity_filter, item_type_filter, name_filter).await?;
        
        Ok(has_new_item)
    }

    //自动打开箱子并清理背包
    pub async fn clear_bag(
        &mut self,
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str],
    ) -> anyhow::Result<()> 
    {
        //FIXME
        //重复操作多了，考虑记住走过的空格，当有开出新箱子的时候往回遍历这些新空格
        let mut count = 0;
        loop {
            count += 1;
            info!("进行第{}轮操作", count);
            //根据是否卖出来判断是否结束
            //先清理背包
            let has_sold = self.sale_bag_items(rarity_filter, item_type_filter, name_filter).await?;
            info!("第{}轮操作是否有物品卖出:{}", count, has_sold);
            //开箱子
            let has_new_item = self.open_bag_item(rarity_filter, item_type_filter, name_filter).await?;
            info!("第{}轮操作是否有新增物品:{}", count, has_new_item);
            //卖出 新增 继续
            //没卖出 新增 继续
            //没卖出 没新增 停止
            //卖出 没新增 停止
            if !has_new_item {
                break;
            }
        }
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
            //点击空白位置关闭界面
            self.adb_device.tap(BACK_POS[0], BACK_POS[1]).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            
            //点击格子
            self.adb_device.tap(*x, *y).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            
            //截图
            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            
             //如果是空格子就跳过
            if let None = self.image_helper.get_template_img_pos_by_name(&input_img, "温馨提示2")? {
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
                    self.adb_device.tap(point.center_x as i32, point.center_y as i32).await?;
                    //等待反应
                    tokio::time::sleep(Duration::from_millis(300)).await;  

                    //点击空白位置关闭界面
                    self.adb_device.tap(BACK_POS[0], BACK_POS[1]).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    //再次点击格子
                    self.adb_device.tap(*x, *y).await?;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    //再次判断，这次一定不会是消耗品
                     //截图
                    let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
                    
                    //如果是空格子就跳过
                    if let None = self.image_helper.get_template_img_pos_by_name(&input_img, "温馨提示2")? {
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
                }else {
                    return Err(anyhow!("找不到使用按钮"));
                }

                //当前位置是不变物品
                need_remove_pos_vec.push(*pos);
            }
        }
        //去除不可变物品位置
        need_remove_pos_vec.into_iter().for_each(|e| {set.remove(&e);});
 
        Ok(())
    }

        //自动打开箱子并清理背包
    pub async fn clear_bag_v2(
        &mut self,
        rarity_filter: &[Rarity],
        item_type_filter: &[ItemType],
        name_filter: &[&str],
    ) -> anyhow::Result<()> 
    {

        let mut empty_grid_set = HashSet::new();
        let pos_vec = get_bag_grid_center_pos_vec();
        let mut i = 0;
        loop {
            if i >= pos_vec.len() {
                break;
            }

            let (x, y) = pos_vec[i];
            //下一格子
            i += 1;

            //点击空白位置关闭界面
            self.adb_device.tap(BACK_POS[0], BACK_POS[1]).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;

            //点击当前格子
            self.adb_device.tap(x, y).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;

            let input_img = image::load_from_memory(&self.adb_device.screencap().await?)?;
            //如果是空格子就跳过
            if let None = self.image_helper.get_template_img_pos_by_name(&input_img, "温馨提示2")? {
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
                    self.adb_device.tap(point.center_x as i32, point.center_y as i32).await?;
                    //等待反应
                    tokio::time::sleep(Duration::from_millis(300)).await;  

                    //前面的空格子可能有物品
                    self.handle_empty_grid(&mut empty_grid_set, rarity_filter, item_type_filter, name_filter).await?;

                    //再次判断当前位置
                    i -= 1;
                }else {
                    return Err(anyhow!("找不到使用按钮"));
                }
            }

            
            //是否是可删除的
            if need_remove(&item_info, rarity_filter, item_type_filter, name_filter) {
                debug!("当前格子可移除");
                self.remove_item(&input_img).await?;
                //当前格子为空，加入空格子列表
                empty_grid_set.insert((x,y));
                continue;
            }
        }
        Ok(())
    }

}

#[cfg(test)]
mod test {
    use tracing::{info, level_filters::LevelFilter};

    use crate::{funs::{GameHelper, ItemType, Rarity}, mumu_manager::VmClient, orc_helper::OcrClient, util};
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
        "屠龙勇士称号"
    ];
    const MANAGER_PATH: &str = r"c:\Users\10401\software\MuMuPlayer\nx_main\MuMuManager.exe";
    const OCR_SERVER_ADDR: &str = "127.0.0.1:9000";

    #[tokio::test]
    async fn test_new_game_helper() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR), None).await.unwrap();
        info!("{:?}", gh);
    }

    #[tokio::test]
    async fn test_click_task_button() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let mut gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR),None).await.unwrap();
        info!("{:?}", gh);
        gh.click_task_button().await.unwrap();
    }

    #[tokio::test]
    async fn test_sale_bag_items() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let mut gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR),None).await.unwrap();
        info!("{:?}", gh);
        gh.sale_bag_items(
            &[Rarity::Common, Rarity::Fine],
            &[ItemType::Equipment],
            &[]
        ).await.unwrap();
    }

    #[tokio::test]
    async fn test_clear_bag() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let mut gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR),None).await.unwrap();
        info!("{:?}", gh);
        gh.clear_bag(
            &[Rarity::Common, Rarity::Fine],
            &[ItemType::Equipment],
            &FILTER_NAMES
        ).await.unwrap();
    }

    #[tokio::test]
    async fn test_clear_bag_v2() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let mut gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR),None).await.unwrap();
        info!("{:?}", gh);
        gh.clear_bag_v2(
            &[Rarity::Common, Rarity::Fine],
            &[ItemType::Equipment],
            &FILTER_NAMES
        ).await.unwrap();
    }

    #[tokio::test]
    async fn screenshot() {
        util::init_logger();
        let vm_client = VmClient::new(0, MANAGER_PATH);
        let gh = GameHelper::new(vm_client, OcrClient::new(OCR_SERVER_ADDR),None).await.unwrap();
        let data = gh.adb_device.screencap().await.unwrap();
        image::load_from_memory(&data)
            .unwrap()
            .save("test_scfeenshot.png")
            .unwrap();
    }
}
