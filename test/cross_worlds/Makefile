include ../test_common.mk

$(BIN): $(C_OBJS) $(CXX_OBJS)
	@cp -r $(C_OBJS) .
	@$(CC) server.o $(LINK_FLAGS) -o $(BIN)_server
	@$(CC) client.o $(LINK_FLAGS) -o $(BIN)_client
	@rm *.o
	@cp $(CUR_DIR)/cross_worlds_test_runner.sh $(BUILD_DIR)/test

test:
	@cd $(BUILD_DIR)/test && \
	./cross_worlds_test_runner.sh libos

test-native:
	@cd $(BUILD_DIR)/test && \
	./cross_worlds_test_runner.sh host
