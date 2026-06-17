use std::thread::sleep;
use std::time::{Duration, Instant};

use super::packet_client::NaverPacketClient;
use super::types::{DiscussionSelection, DiscussionStock};
use super::{AutomationResult, CdpClient};

impl CdpClient {
    // 패킷 API로 랜덤 종목을 선택하고 Chrome 화면을 해당 종목 토론방 URL로 이동시키는 함수입니다.
    pub(super) fn open_random_discussion_room(
        &mut self,
        packet_client: &NaverPacketClient,
    ) -> AutomationResult<DiscussionSelection> {
        self.handle_npay_agreement_if_present()?;
        let room = packet_client.select_random_discussion_room()?;
        self.navigate(&room.discussion_url)?;
        self.wait_for_stock_discussion_url(Duration::from_secs(12))?;
        sleep(Duration::from_secs(1));
        Ok(room.selection)
    }

    // UI에서 선택한 종목 코드로 토론방 URL을 만들고 Chrome 화면을 이동시키는 함수입니다.
    pub(super) fn open_selected_discussion_room(
        &mut self,
        stock: &DiscussionStock,
    ) -> AutomationResult<DiscussionSelection> {
        self.handle_npay_agreement_if_present()?;
        let discussion_url = format!(
            "https://stock.naver.com/domestic/stock/{}/discussion?chip=all",
            stock.code.trim()
        );
        self.navigate(&discussion_url)?;
        self.wait_for_stock_discussion_url(Duration::from_secs(12))?;
        sleep(Duration::from_secs(1));

        Ok(DiscussionSelection {
            category: "사용자 선택".to_owned(),
            rank: "-".to_owned(),
            item_text: format!("{} ({})", stock.name.trim(), stock.code.trim()),
            method: "ui-selected-stock".to_owned(),
        })
    }

    // 패킷 API로 현재 종목의 랜덤 토론글을 선택하고 Chrome 화면을 해당 게시글 URL로 이동시키는 함수입니다.
    pub(super) fn open_random_discussion_post(
        &mut self,
        packet_client: &NaverPacketClient,
    ) -> AutomationResult<()> {
        let current_url = self.current_url()?;
        let post = packet_client.select_random_discussion_post(&current_url)?;
        self.navigate(&post.post_url)?;
        self.wait_for_stock_discussion_url(Duration::from_secs(12))?;
        sleep(Duration::from_secs(1));
        Ok(())
    }

    // 종목별 토론방 또는 토론글 URL로 이동이 끝났는지 기다리는 함수입니다.
    fn wait_for_stock_discussion_url(&mut self, timeout: Duration) -> AutomationResult<()> {
        let end = Instant::now() + timeout;

        while Instant::now() < end {
            let url = self.current_url()?;

            if url.contains("/domestic/stock/")
                || url.contains("/domestic/index/")
                || url.contains("/worldstock/stock/")
                || url.contains("/worldstock/index/")
            {
                return Ok(());
            }

            sleep(Duration::from_millis(500));
        }

        Ok(())
    }
}
