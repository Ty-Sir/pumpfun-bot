use chrono::Local;
use std::sync::Arc;
use std::time::Duration;

use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signer::Signer};
use spl_associated_token_account;
// use spl_token::instruction::close_account;

use super::constants::*;
use super::create_ix::{create_sell_ix, get_buy_ix};
use super::pf_price::*;

use crate::txn::spam_txn::spammer;

use super::layouts::{CreateEvent, TradeEvent};

async fn valid_logs(logs: &Vec<String>) -> bool {
    let mut a = false;
    let mut b = false;
    for msg in logs {
        if msg.contains("InitializeMint2") || msg.contains("Create Metadata Accounts v3") {
            a = true;
        } else if msg.contains("Buy") {
            b = true;
            break;
        }
    }

    if a && b {
        return true;
    }

    return false;
}

pub async fn process_logs(
    logs: &Vec<String>,
    client: Arc<RpcClient>,
    payer: Arc<Keypair>,
    investment_lamported: f64,
    slippage: f64,
    adjusted_investment_for_fees: f64,
    unit_limit_ix: Instruction,
    prices_4_spam: Vec<Instruction>,
    m_pk: &Pubkey,
) {
    let mut mint = Pubkey::default();
    let mut bc_pk = Pubkey::default();
    let mut user = Pubkey::default();
    let mut virtual_sol_reserves = 0;
    let mut virtual_token_reserves = 0;

    if valid_logs(logs).await {
        for log in logs {
            if log.contains("Program data:") {
                let log_data = log.replace("Program data: ", "");
                let log_decoded = base64::decode(&log_data).expect("Failed to decode base64");

                // if &log_decoded[8..].len() == &208 { or expect CreateEvent size
                // or base64 log starts with G3KpTd7r
                if log.contains("G3KpTd7r") {
                    let create_event = CreateEvent::decode_create_event(&log_decoded[8..]);
                    bc_pk = create_event.bonding_curve;
                }
                // if &log_decoded[8..].len() == &121 { or expect TradeEvent size
                // or base64 log starts with vdt/007mYe
                if log.contains("vdt/007mYe") {
                    let trade_event = TradeEvent::decode_trade_event(&log_decoded[8..]);
                    virtual_sol_reserves = trade_event.get_virtual_sol_reserves();
                    virtual_token_reserves = trade_event.get_virtual_token_reserves();
                    mint = trade_event.mint;
                    user = trade_event.user;
                }
            }
        }
    }

    // check and send,,,,,,,,
    if user != Pubkey::default()
        && mint != Pubkey::default()
        && bc_pk != Pubkey::default()
        && virtual_sol_reserves > 0
        && virtual_token_reserves > 0
    {
        println!(
            "{}:: user: {:?} \nmint: {:?}",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            user,
            mint
        );
        // println!("-----------------");

        let bc_pk_ata = Pubkey::find_program_address(
            &[
                &bc_pk.to_bytes(),
                &TOKEN_PROGRAM_ID.to_bytes(),
                &mint.to_bytes(),
            ],
            &ASSOCIATED_TOKEN_PROGRAM_ID,
        )
        .0;

        // println!("BC ATA:{}", &bc_pk_ata);

        // price and tokens calcualtion

        let final_with_slippage_int = get_sol2tokens(
            virtual_sol_reserves,
            virtual_token_reserves,
            investment_lamported,
            slippage,
        )
        .await
        .expect("Failed to get price, terminating program.");

        println!("final_with_slippage_int: {}", final_with_slippage_int);

        // --------------------------------
        //create token ata.
        let mint_ata =
            spl_associated_token_account::get_associated_token_address(&payer.pubkey(), &mint);

        let ix_ata: Instruction =
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint,
                &TOKEN_PROGRAM_ID,
            );

        // buy ix-----------
        let buy_ix = get_buy_ix(
            final_with_slippage_int as u64,
            adjusted_investment_for_fees as u64,
            mint,
            bc_pk,
            bc_pk_ata,
            mint_ata,
            payer.as_ref(),
        )
        .unwrap();

        // tx info--------------------
        let ixs: Vec<Instruction> = vec![ix_ata, buy_ix, unit_limit_ix.clone()];

        spammer(prices_4_spam.clone(), &client, &payer, &m_pk, &ixs).await;

        // // incase you wanted to exit on specific profits......  not fully implemented
        // let mut account_token_balance = 0;
        // let start_checking_balance = Instant::now();
        // loop {
        //     match client
        //         .get_token_account_balance_with_commitment(&mint_ata, CommitmentConfig::processed())
        //         .await
        //     {
        //         Ok(account_balance) => {
        //             let amount: u64 = account_balance.value.amount.parse::<u64>().unwrap_or(0);
        //             account_token_balance = amount;
        //             if amount > 0 {
        //                 println!(
        //                     "{}::Balance found: {:?}",
        //                     Local::now().format("%Y-%m-%d %H:%M:%S"),
        //                     &amount
        //                 );

        //                 let duration = start_checking_balance.elapsed(); //current time
        //                 println!("Time Consumed to amount shown in account: {:?}", duration); //print it
        //                 break;
        //             }
        //         }
        //         Err(e) => {
        //             tokio::time::sleep(Duration::from_millis(100)).await;
        //             // println!("Failed to fetch balance: {:?}", e);
        //             // if start_checking_balance.elapsed() > Duration::from_secs(20) {
        //             //     println!("Balance not found");
        //             //     break;
        //             // }
        //         }
        //     }
        // }

        //----------------------------------------------------------------
        //----------------------------------------------------------------
        println!("Going to sleep"); //----------------------------------------------------------------
        tokio::time::sleep(Duration::from_secs(10)).await;
        //----------------------------------------------------------------
        //----------------------------------------------------------------
        //----------------------------------------------------------------

        let sell_ix = create_sell_ix(
            final_with_slippage_int as u64,
            0 as u64,
            mint,
            bc_pk,
            bc_pk_ata,
            mint_ata,
            payer.as_ref(),
        )
        .unwrap();

        // let close_acc_ix = close_account(
        //     &TOKEN_PROGRAM_ID,
        //     &mint_ata,
        //     &payer.pubkey(),
        //     &payer.pubkey(),
        //     &[&payer.pubkey()],
        // )
        // .unwrap();

        let ixs_sell: Vec<Instruction> = vec![sell_ix, unit_limit_ix.clone()];

        //          let recent_blockhash1 = client.get_latest_blockhash_with_commitment(CommitmentConfig::processed()).await.unwrap(); //get blockhash
        //        let tx = Transaction::new_signed_with_payer(&ixs_sell,Some(&payer.pubkey()), &[&payer], recent_blockhash1.0);
        //            let sig = client.send_transaction(&tx).await.unwrap();
        //   println!("sig: {}",&sig.to_string());

        println!("going to spam sell");
        spammer(prices_4_spam.to_vec(), &client, &payer, &m_pk, &ixs_sell).await;

        println!("{}::DOne", Local::now().format("%Y-%m-%d %H:%M:%S"));
        println!("------------------------------------------------------------------");
    }
}
