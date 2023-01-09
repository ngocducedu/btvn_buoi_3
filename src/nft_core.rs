use crate::*;

const NO_DEPOSIT:Balance = 0;
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas(10_000_000_000_000);
const GAS_FOR_NFT_TRANSFER_CALL: Gas = Gas(35_000_000_000_000);

pub trait NonFungibleTokenCore {
    fn nft_transfer(&mut self, receiver_id: AccountId, token_id: TokenId, approval_id: u64, memo: Option<String>);

    // return true neu transfer NFT duoc thuc hien thanh cong
    fn nft_transfer_call(&mut self, receiver_id: AccountId, token_id: TokenId, approval_id: u64, memo: Option<String>, msg: String) -> PromiseOrValue<bool>;
}

#[ext_contract(ext_non_fungible_token_receiver)]
trait NonFungibleTokenReceiver   {
    // Method luu tren Contract B, A thuc hien cross contract call nft_on_transfer
    // return true neu nhu NFT can duoc rollback lai cho owner cu
    fn nft_on_transfer(&mut self, sender_id: AccountId, previous_owner_id: AccountId, token_id: TokenId, msg: String) -> Promise;
}

#[ext_contract(ext_self)]
trait NonFungibleTokenResolver {
    // Neu contract B yeu cau rollback lai cho owner cu => A se rollback lai data trong nft_resolve_transfer
    fn nft_resolve_transfer(
        &mut self,
        authorized_id: Option<AccountId>,
        owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: HashMap<AccountId, u64>,
        memo: Option<String>
    ) -> bool;
}

#[near_bindgen]
impl NonFungibleTokenCore for Contract {
    // Yeu cau deposit 1 yoctoNear de bao mat cho user
    #[payable]
    fn nft_transfer(&mut self, receiver_id: AccountId, token_id: TokenId, approval_id: u64, memo: Option<String>) {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();

        let previous_token = self.internal_transfer(
            &sender_id,
            &receiver_id,
            &token_id,
            Some(approval_id),
            memo
        );

        refund_approved_account_ids(sender_id, &previous_token.approved_account_ids);
    }

    #[payable]
    fn nft_transfer_call(&mut self, receiver_id: AccountId, token_id: TokenId, approval_id: u64, memo: Option<String>, msg:String) -> PromiseOrValue<bool> {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();

        let previous_token = self.internal_transfer(
            &sender_id, 
            &receiver_id, 
            &token_id, 
            Some(approval_id), 
            memo.clone()
        );

        let mut authorized_id  = None;
        if  sender_id != previous_token.owner_id {
            authorized_id = Some(sender_id.clone());
        }
        
        ext_non_fungible_token_receiver::ext(receiver_id.clone())
            .with_static_gas(env::prepaid_gas() - GAS_FOR_NFT_TRANSFER_CALL)
            .nft_on_transfer( 
                sender_id, 
                previous_token.owner_id.clone(), 
                token_id.clone(), 
                msg
            ).then(ext_self::ext(env::current_account_id())
            .with_static_gas(GAS_FOR_RESOLVE_TRANSFER)
            .nft_resolve_transfer(
                authorized_id,
                previous_token.owner_id, 
                receiver_id.clone(), 
                token_id, 
                previous_token.approved_account_ids,
                memo,
            )).into()
        

    }
}

#[near_bindgen]
impl NonFungibleTokenResolver for Contract {
    fn nft_resolve_transfer(
        &mut self,
        authorized_id: Option<AccountId>,
        owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: HashMap<AccountId, u64>,
        memo: Option<String>
    ) -> bool {

        // truong hop promise thanh cong, ko can rollback lai token

        if let PromiseResult::Successful(value) = env::promise_result(0) {
            if let Ok(is_rollback_token) = near_sdk::serde_json::from_slice::<bool>(&value) {
                return !is_rollback_token;
            }
        }

        // check trong hop token co ton tai hay khong
        let mut token = if let Some(token) = self.tokens_by_id.get(&token_id) {
            // truong hop co token ton tai. Neu owner cua token khac receiver thi k can rollback
            if token.owner_id != receiver_id {
                refund_approved_account_ids(owner_id, &approved_account_ids);
                return true;
            }
            token
        } else {
            // truong hop ko co token thi return true
            refund_approved_account_ids(owner_id, &approved_account_ids);
            return true;
        };


        // cac truong hop can rollback lai token
        log!("Rollback {} from @{} to @{}", token_id, receiver_id, owner_id);
        
        self.internal_remove_token_from_owner(&token_id, &receiver_id);
        self.internal_add_token_to_owner(&token_id, &owner_id);

        token.owner_id = owner_id.clone();

        refund_approved_account_ids(receiver_id.clone(),  &token.approved_account_ids);
        token.approved_account_ids = approved_account_ids;

        self.tokens_by_id.insert(&token_id, &token);
        
        // NFT TRANSFER LOG
        let nft_transfer_log = EventLog {
            standard: "nep171".to_string(),
            version: "1.0.0".to_string(),
            event: EventLogVariant::NftTransfer(vec![ NftTransferLog {
                authorized_id: Some(authorized_id.unwrap().to_string()),
                old_owner_id: receiver_id.to_string(),
                new_owner_id: owner_id.to_string(),
                token_ids: vec![token_id.to_string()],
                memo
            } ])
        };

        env::log(&nft_transfer_log.to_string().as_bytes());

        // return false de frontend biet transfer call function nay that bai
        false

    }
}
