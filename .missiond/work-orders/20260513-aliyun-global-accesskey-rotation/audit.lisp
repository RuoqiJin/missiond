(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :work_order_id wo-20260513-aliyun-global-accesskey-rotation
  :events
    ((event e1 :kind input-received :summary "User provided three revoked Aliyun AccessKey ids and one new /Users/jinchen/Downloads/AccessKey.csv file; clarified the new key is global Aliyun, not DNS-only.")
     (event e2 :kind secret-store-write :summary "Created or used secret-store namespace aliyun-global and wrote ALIYUN_ACCESS_KEY_ID plus ALIYUN_ACCESS_KEY_SECRET without printing values.")
     (event e3 :kind cleanup :summary "Removed mistaken CHANGTU_PRO_ACCESS_KEY_ID and CHANGTU_PRO_ACCESS_KEY_SECRET entries from aliyun-dns namespace; namespace remains empty.")
     (event e4 :kind verification :summary "Aliyun CLI DescribeDomainRecords for changtu.pro succeeded with record_count=0 using one-shot environment credentials.")
     (event e5 :kind ssot-update :summary "MissionD infrastructure universe now has aliyun-account and aliyun-dns references to secret-store://aliyun-global/* plus cloud-ops-delegation-policy.")
     (event e6 :kind evidence-update :summary "Secret-store service intent and evidence now record MissionD deploy-ops use of aliyun-global namespace."))
  :open_items
    ((item cn-secret-store-deploy :summary "Secret-store CN runtime must be verified/deployed through deploy-center before claiming cn-production secret-store is fully governed.")
     (item aliyun-oss-provisioning :summary "OSS bucket creation for CN object storage still requires explicit mutation work-order approval.")
     (item work-order-board-entry :summary "Future repeats should create the BoardTask before execution so Board and intent sources share one visible anchor."))
  :redaction "No AccessKey secret value is stored in this audit.")
